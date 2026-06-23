//! Newtype identifiers. Every id is `Copy`, totally ordered, and serde-able — the total order
//! is load-bearing for the determinism tie-breaks in `resolve` (see `PORTING.md` §4).

use serde::{Deserialize, Serialize};

macro_rules! id_newtype {
    ($name:ident, $doc:literal) => {
        #[doc = $doc]
        #[derive(
            Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Serialize, Deserialize,
        )]
        pub struct $name(pub u32);

        impl $name {
            #[inline]
            pub fn raw(self) -> u32 {
                self.0
            }
        }
    };
}

id_newtype!(ActorId, "Identifies one combatant.");
id_newtype!(
    FactionId,
    "A side in the fight. `0` = Player, `1` = Enemy (extensible)."
);
id_newtype!(
    InstanceId,
    "One scheduled action on the timeline; monotonic, assigned by `Sim`."
);
id_newtype!(MoveId, "One entry in the `MoveLibrary`.");
id_newtype!(
    WindowId,
    "One timed tag (a window) on an actor; monotonic, assigned by `Sim`."
);

impl FactionId {
    pub const PLAYER: FactionId = FactionId(0);
    pub const ENEMY: FactionId = FactionId(1);
}
