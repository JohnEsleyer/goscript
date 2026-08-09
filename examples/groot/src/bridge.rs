use std::collections::HashMap;
use std::rc::Rc;

use goscript::hot_reload::HotReloadEngine;

// ---------------------------------------------------------------------------
// Entity identity
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EntityId(pub u64);

// ---------------------------------------------------------------------------
// Input snapshot — synced from the "host" (main loop) into every VM each tick.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
pub struct InputState {
    pub keys_down: Vec<String>,
    pub keys_pressed: Vec<String>,
    pub mouse_x: f64,
    pub mouse_y: f64,
    pub mouse_pressed: bool,
}

// ---------------------------------------------------------------------------
// Deferred commands issued by scripts, processed after all scripts run.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum EngineCommand {
    SpawnEntity {
        script: String,
        x: f64,
        y: f64,
    },
    DestroyEntity(EntityId),
    SetPosition {
        entity: EntityId,
        x: f64,
        y: f64,
    },
    PlaySound {
        name: String,
    },
}

// ---------------------------------------------------------------------------
// ScriptBridgeState — the shared world state that the ECS and scripting
// engines both read/write.  The `GrootScriptHost` owns this behind a
// `RefCell` so each per-entity VM can mutate it through registered host fns.
// ---------------------------------------------------------------------------

pub struct ScriptBridgeState {
    /// Monotonically increasing entity ID generator.
    next_id: u64,
    /// All live entity positions, keyed by EntityId.
    pub positions: HashMap<EntityId, (f64, f64)>,
    /// The current frame's input snapshot.
    pub input: InputState,
    /// Queue of commands issued by scripts this tick.
    pub commands: Vec<EngineCommand>,
    /// Map of entity ID -> script path (for diagnostics / reload).
    pub entity_scripts: HashMap<EntityId, String>,
}

impl ScriptBridgeState {
    pub fn new() -> Self {
        Self {
            next_id: 1,
            positions: HashMap::new(),
            input: InputState::default(),
            commands: Vec::new(),
            entity_scripts: HashMap::new(),
        }
    }

    pub fn spawn_id(&mut self) -> EntityId {
        let id = EntityId(self.next_id);
        self.next_id += 1;
        id
    }
}

// ---------------------------------------------------------------------------
// GrootScriptHost — owns one HotReloadEngine per entity, keyed by EntityId.
// ---------------------------------------------------------------------------

pub struct GrootScriptHost {
    pub engines: HashMap<EntityId, HotReloadEngine>,
}

impl GrootScriptHost {
    pub fn new() -> Self {
        Self {
            engines: HashMap::new(),
        }
    }

    pub fn spawn_entity(
        &mut self,
        id: EntityId,
        script_path: &str,
        bridge: Rc<std::cell::RefCell<ScriptBridgeState>>,
    ) {
        let mut engine = HotReloadEngine::new(script_path);
        // Register the full Groot API on this engine's VM.
        super::module::register_groot_api(&mut engine.vm, id, bridge);
        self.engines.insert(id, engine);
    }

    pub fn remove_entity(&mut self, id: EntityId) {
        self.engines.remove(&id);
    }
}
