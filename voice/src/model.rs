//! The candle adapter: a quantized Qwen2.5-1.5B-Instruct loaded once and run on CPU
//! (GPU strictly opt-in), implementing the simulation's [`agents::TextGen`] seam.
//!
//! This is the only place that touches `candle`. It turns a fully-assembled prompt into a
//! short line and nothing more — all the grounding, caching, and grammar fallback live a
//! layer up (see `lib.rs`). On any error it yields an empty string, which the caller treats
//! as "fall back to the grammar surface" — the model is never load-bearing (`docs/dialogue.md`).

use anyhow::{Context, Result, anyhow};
use candle_core::quantized::gguf_file;
use candle_core::{Device, Tensor};
use candle_transformers::generation::LogitsProcessor;
use candle_transformers::models::quantized_qwen2::ModelWeights;
use config::VoiceConfig;
use std::cell::RefCell;
use hf_hub::api::sync::Api;
use tokenizers::Tokenizer;

/// A loaded on-device model. Owns the weights, tokenizer, and sampling knobs. The weights
/// sit behind a `RefCell` because generation mutates the per-layer KV cache (`forward` takes
/// `&mut self`) while the [`agents::TextGen`] seam hands us `&self`; the model lives on a
/// single worker thread, so a cell is the right tool (it is `Send` but not `Sync`).
pub struct CandleModel {
    model: RefCell<ModelWeights>,
    tokenizer: Tokenizer,
    device: Device,
    /// Token ids that end a turn (`<|im_end|>` / `<|endoftext|>`), looked up from the tokenizer.
    eos: Vec<u32>,
    temperature: f64,
    max_tokens: usize,
}

/// CPU by default — no VRAM contention with the Bevy renderer. GPU backends are opt-in and
/// fall back to CPU if a device can't be acquired.
fn pick_device() -> Device {
    #[cfg(feature = "cuda")]
    if let Ok(d) = Device::new_cuda(0) {
        return d;
    }
    #[cfg(feature = "metal")]
    if let Ok(d) = Device::new_metal(0) {
        return d;
    }
    Device::Cpu
}

impl CandleModel {
    /// Fetch the weights + tokenizer (HuggingFace Hub, cached under `~/.cache/huggingface`)
    /// and build the model. Blocking and slow (~1–2 s + a one-time multi-hundred-MB download
    /// on first run) — call it off the main thread.
    pub fn load(cfg: &VoiceConfig) -> Result<Self> {
        let device = pick_device();

        let api = Api::new().context("create HuggingFace Hub client")?;
        let gguf_path = api
            .model(cfg.repo.clone())
            .get(&cfg.gguf_file)
            .with_context(|| format!("fetch '{}' from '{}'", cfg.gguf_file, cfg.repo))?;
        let tok_path = api
            .model(cfg.tokenizer_repo.clone())
            .get("tokenizer.json")
            .with_context(|| format!("fetch tokenizer.json from '{}'", cfg.tokenizer_repo))?;

        let tokenizer = Tokenizer::from_file(&tok_path).map_err(|e| anyhow!("load tokenizer: {e}"))?;

        let mut file = std::fs::File::open(&gguf_path).with_context(|| format!("open {gguf_path:?}"))?;
        let content = gguf_file::Content::read(&mut file).with_context(|| format!("read GGUF {gguf_path:?}"))?;
        let model = ModelWeights::from_gguf(content, &mut file, &device).context("build model from GGUF")?;

        let eos = ["<|im_end|>", "<|endoftext|>"].iter().filter_map(|t| tokenizer.token_to_id(t)).collect();

        Ok(Self { model: RefCell::new(model), tokenizer, device, eos, temperature: cfg.temperature, max_tokens: cfg.max_tokens })
    }

    /// The greedy/seeded autoregressive loop. `prompt` is the fully-formatted ChatML string;
    /// the returned text is the assistant turn only, trimmed. Errors propagate so the public
    /// [`agents::TextGen`] wrapper can fall back to the grammar.
    fn run(&self, prompt: &str, seed: u64) -> Result<String> {
        let encoding = self.tokenizer.encode(prompt, false).map_err(|e| anyhow!("encode: {e}"))?;
        let prompt_ids = encoding.get_ids();
        if prompt_ids.is_empty() {
            return Ok(String::new());
        }

        // `temperature <= 0` → deterministic argmax (reproducible on a machine, per the doc).
        let temperature = (self.temperature > 0.0).then_some(self.temperature);
        let mut sampler = LogitsProcessor::new(seed, temperature, None);
        let mut model = self.model.borrow_mut();

        // Prefill: feeding the whole prompt at index_pos 0 also resets the KV cache left by
        // any previous generation, so the model is safe to reuse across calls.
        let input = Tensor::new(prompt_ids, &self.device)?.unsqueeze(0)?;
        let logits = model.forward(&input, 0)?.squeeze(0)?;
        let mut next = sampler.sample(&logits)?;

        let mut out: Vec<u32> = Vec::with_capacity(self.max_tokens);
        let mut pos = prompt_ids.len();
        for _ in 0..self.max_tokens {
            if self.eos.contains(&next) {
                break;
            }
            out.push(next);
            let input = Tensor::new(&[next], &self.device)?.unsqueeze(0)?;
            let logits = model.forward(&input, pos)?.squeeze(0)?;
            next = sampler.sample(&logits)?;
            pos += 1;
        }

        let text = self.tokenizer.decode(&out, true).map_err(|e| anyhow!("decode: {e}"))?;
        Ok(text.trim().to_string())
    }
}

impl agents::TextGen for CandleModel {
    /// Generate a line, deterministically given `seed`. An empty return means "use the
    /// grammar surface" — the realizer's guard handles it. Errors are logged, never panic.
    fn generate(&self, prompt: &str, seed: u64) -> String {
        match self.run(prompt, seed) {
            Ok(line) => line,
            Err(e) => {
                eprintln!("voice: generation failed, falling back to grammar: {e:#}");
                String::new()
            }
        }
    }
}
