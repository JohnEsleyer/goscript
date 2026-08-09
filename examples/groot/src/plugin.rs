use std::cell::RefCell;
use std::rc::Rc;

use goscript::value::Value;

use crate::bridge::{EngineCommand, GrootScriptHost, ScriptBridgeState};

/// Syncs the shared input snapshot into each entity's VM globals so scripts
/// can read `Input.MouseX`, `Input.KeysDown`, etc. via the host functions.
/// (The host functions already read from the bridge, so this is mostly a
/// no-op placeholder for future per-frame global injection.)
pub fn system_sync_input(
    _host: &mut GrootScriptHost,
    _bridge: &Rc<RefCell<ScriptBridgeState>>,
) {
    // Input is already live in ScriptBridgeState; host fns read it directly.
}

/// Runs hot-reload + OnUpdate for every entity script.
pub fn system_run_scripts(
    host: &mut GrootScriptHost,
    bridge: &Rc<RefCell<ScriptBridgeState>>,
    dt: f64,
) {
    let entity_ids: Vec<_> = host.engines.keys().copied().collect();
    for eid in entity_ids {
        let engine = match host.engines.get_mut(&eid) {
            Some(e) => e,
            None => continue,
        };

        // Hot-reload: recompile if the .gs file changed on disk.
        if let Ok(true) = engine.reload_if_changed() {
            let script = bridge
                .borrow()
                .entity_scripts
                .get(&eid)
                .cloned()
                .unwrap_or_default();
            println!("[groot] entity {:?} script reloaded: {}", eid, script);
        }

        // Set frame delta so time.Delta() works inside scripts.
        engine.vm.set_delta_time(dt);

        // Call OnUpdate(dt) if the script defines it.
        let _ = engine.vm.call("OnUpdate", vec![Value::Float(dt)]);
    }
}

/// Processes deferred commands emitted by scripts during the current tick.
pub fn system_process_commands(
    host: &mut GrootScriptHost,
    bridge: &Rc<RefCell<ScriptBridgeState>>,
) {
    let commands: Vec<EngineCommand> = bridge.borrow_mut().commands.drain(..).collect();

    for cmd in commands {
        match cmd {
            EngineCommand::SpawnEntity { script, x, y } => {
                let mut st = bridge.borrow_mut();
                let new_id = st.spawn_id();
                st.positions.insert(new_id, (x, y));
                st.entity_scripts.insert(new_id, script.clone());
                drop(st);
                host.spawn_entity(new_id, &script, Rc::clone(bridge));
                println!("[groot] spawned entity {:?} from '{}'", new_id, script);
            }
            EngineCommand::DestroyEntity(eid) => {
                host.remove_entity(eid);
                bridge.borrow_mut().positions.remove(&eid);
                bridge.borrow_mut().entity_scripts.remove(&eid);
                println!("[groot] destroyed entity {:?}", eid);
            }
            EngineCommand::SetPosition { entity, x, y } => {
                bridge.borrow_mut().positions.insert(entity, (x, y));
            }
            EngineCommand::PlaySound { name } => {
                println!("[groot] PlaySound: {}", name);
            }
        }
    }
}
