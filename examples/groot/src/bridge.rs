use std::collections::HashMap;
use std::rc::Rc;

use goscript::hot_reload::HotReloadEngine;

use crate::groot_module::GrootModuleExt;

// ---------------------------------------------------------------------------
// Entity identity
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EntityId(pub u64);

// ---------------------------------------------------------------------------
// Per-entity runtime state — managed by the plugin, read/written by scripts.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct EntityState {
    pub x: f64,
    pub y: f64,
    pub rotation: f64,
    pub scale_x: f64,
    pub scale_y: f64,
    pub color: (f64, f64, f64, f64),
    pub destroy_requested: bool,
}

impl Default for EntityState {
    fn default() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            rotation: 0.0,
            scale_x: 1.0,
            scale_y: 1.0,
            color: (0.1, 0.8, 0.3, 1.0),
            destroy_requested: false,
        }
    }
}

// ---------------------------------------------------------------------------
// Input snapshot — synced from the "host" (main loop) into every VM each tick.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
pub struct InputState {
    pub keys_down: Vec<String>,
    pub keys_pressed: Vec<String>,
    pub keys_released: Vec<String>,
    pub mouse_x: f64,
    pub mouse_y: f64,
    pub mouse_button_down: [bool; 3],
    pub mouse_button_pressed: [bool; 3],
}

// ---------------------------------------------------------------------------
// Debug drawing commands issued by scripts (Raylib-style immediate draw).
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum DebugDrawCommand {
    Line {
        x1: f64,
        y1: f64,
        x2: f64,
        y2: f64,
        r: f64,
        g: f64,
        b: f64,
    },
    Circle {
        cx: f64,
        cy: f64,
        radius: f64,
        r: f64,
        g: f64,
        b: f64,
    },
    Rect {
        cx: f64,
        cy: f64,
        w: f64,
        h: f64,
        r: f64,
        g: f64,
        b: f64,
    },
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
    PlaySound {
        name: String,
    },
}

// ---------------------------------------------------------------------------
// Script event bus — inter-script communication.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct ScriptEvent {
    pub event_name: String,
    pub payload: f64,
}

// ---------------------------------------------------------------------------
// ScriptBridgeState — the shared world state.
// ---------------------------------------------------------------------------

pub struct ScriptBridgeState {
    next_id: u64,
    /// Per-entity runtime state (position, rotation, scale, color).
    pub entity_states: HashMap<EntityId, EntityState>,
    /// The current frame's input snapshot.
    pub input: InputState,
    /// Queue of commands issued by scripts this tick.
    pub commands: Vec<EngineCommand>,
    /// Queue of debug draw commands issued by scripts this tick.
    pub debug_draws: Vec<DebugDrawCommand>,
    /// Event bus for inter-script communication.
    pub events: Vec<ScriptEvent>,
    /// Map of entity ID -> script path.
    pub entity_scripts: HashMap<EntityId, String>,
}

impl ScriptBridgeState {
    pub fn new() -> Self {
        Self {
            next_id: 1,
            entity_states: HashMap::new(),
            input: InputState::default(),
            commands: Vec::new(),
            debug_draws: Vec::new(),
            events: Vec::new(),
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
// GrootScriptHost — owns one HotReloadEngine per script file (shared across
// entities using the same script). Entity context is set per-tick.
// ---------------------------------------------------------------------------

pub struct GrootScriptHost {
    pub engines: HashMap<String, HotReloadEngine>,
}

impl GrootScriptHost {
    pub fn new() -> Self {
        Self {
            engines: HashMap::new(),
        }
    }

    pub fn ensure_engine_loaded(
        &mut self,
        script_path: &str,
        bridge: Rc<std::cell::RefCell<ScriptBridgeState>>,
    ) -> &mut HotReloadEngine {
        self.engines
            .entry(script_path.to_string())
            .or_insert_with(|| {
                let mut engine = HotReloadEngine::new(script_path);
                engine.vm.register_groot_module();
                super::groot_plugin::register_entity_api(&mut engine.vm, bridge);
                engine
            })
    }
}
