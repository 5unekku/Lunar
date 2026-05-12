# engine_zones

world zone management

zones are collections of entities and systems that can be loaded/unloaded independently.
the world persists across zone transitions, enabling seamless RPG-style area changes.


## Structs

### FadeConfig

fade configuration for zone transitions.

controls the visual effect when moving between zones.

### WorldManager

world manager resource, manages zone loading and transitions.

register zones with [`WorldManager::register_zone`] and transition
between them with [`WorldManager::enter_zone`]. the world state
persists across transitions, allowing seamless area changes.

### ZoneTransition

a transition point that triggers when an entity enters the area.

define these in [`Zone::transitions`] to create automatic area changes
when the player walks into a trigger zone.

## Traits

### Zone

zone trait — implement to define a world zone.

each zone type defines its own lifecycle hooks for loading,
entering, and exiting. implement this trait to create custom zones.
