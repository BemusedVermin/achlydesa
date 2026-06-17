# Design

The simulation is designed to support extensible authored actions on authored substrates. The core components of the design include:

- **Substrate Trait**: This trait defines the interface for substrates, which are the entities on which actions can be performed. It includes methods and associated types that allow for interaction with the substrate.
- **Action Trait**: This trait defines the interface for actions, which are the operations that can be performed on substrates. It includes methods for executing the action and any necessary associated types.
- **Actor Trait**: This trait defines the interface for actors, which are the entities that perform actions on substrates. It includes methods for selecting and executing actions.
- **Simulation Trait**: This trait defines the interface for the simulation itself, which manages the execution of actions on substrates over time. It includes methods for advancing the simulation and managing the state of the system.

## Extensibility

The trait design allows for extensibility by enabling users to implement their own versions of substrates, actions, and actors. This means that new types of substrates and actions can be added without modifying the core simulation logic, making it flexible and adaptable to a wide range of scenarios.

Additionally, this crate provides a default implementation of the traits.

This crate is intended for use in the larger Achlydesa project.