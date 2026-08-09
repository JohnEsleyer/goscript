use std::cell::RefCell;
use std::rc::Rc;

use goscript::value::Value;
use goscript::vm::VirtualMachine;

use crate::bridge::{
    DebugDrawCommand, EngineCommand, EntityId, EntityState, GrootScriptHost, ScriptBridgeState,
    ScriptEvent,
};

// The entity ID currently being executed by the VM. Set before each
// `OnUpdate` call so that `groot.GetSelf*` / `groot.SetSelf*` functions
// operate on the correct entity.
thread_local! {
    static CURRENT_ENTITY: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// Registers the per-entity API functions on a VM. These functions use
/// `CURRENT_ENTITY` to dispatch reads/writes to the correct entity's state
/// in `ScriptBridgeState`.
///
/// # API Categories
///
/// 1. **Self Context** — `groot.GetSelfPosition()`, `groot.SetSelfPosition()`, etc.
/// 2. **Entity Queries** — `groot.GetEntityPosition(id)`, `groot.GetDistance(id1, id2)`
/// 3. **Input** — `groot.GetAxis()`, `groot.IsKeyDown()`, `groot.GetMousePosition()`
/// 4. **Debug Drawing** — `groot.DrawDebugLine()`, `groot.DrawDebugCircle()`, `groot.DrawDebugRect()`
/// 5. **Commands** — `groot.SpawnEntity()`, `groot.DestroySelf()`, `groot.PlaySound()`
/// 6. **Event Bus** — `groot.EmitEvent(name, payload)`
pub fn register_entity_api(vm: &mut VirtualMachine, bridge: Rc<RefCell<ScriptBridgeState>>) {
    // ==================================================================
    // 1. Self Context — reads/writes the CURRENT entity's state.
    // ==================================================================

    // groot.GetSelfEntity() -> int64
    vm.register_fn("groot.GetSelfEntity", |_| {
        let id = CURRENT_ENTITY.with(|c| c.get());
        Value::Int(id as i64)
    });

    // groot.GetSelfPosition() -> [x, y]
    let b = Rc::clone(&bridge);
    vm.register_fn("groot.GetSelfPosition", move |_| {
        let id = CURRENT_ENTITY.with(|c| c.get());
        let st = b.borrow();
        let state = st.entity_states.get(&EntityId(id)).cloned().unwrap_or_default();
        Value::Slice(Rc::new(RefCell::new(vec![
            Value::Float(state.x),
            Value::Float(state.y),
        ])))
    });

    // groot.SetSelfPosition(x, y)
    let b = Rc::clone(&bridge);
    vm.register_fn("groot.SetSelfPosition", move |args| {
        let id = CURRENT_ENTITY.with(|c| c.get());
        let nx = args.get(0).and_then(|v| v.as_number()).unwrap_or(0.0);
        let ny = args.get(1).and_then(|v| v.as_number()).unwrap_or(0.0);
        let mut st = b.borrow_mut();
        let state = st.entity_states.entry(EntityId(id)).or_default();
        state.x = nx;
        state.y = ny;
        Value::Nil
    });

    // groot.GetSelfRotation() -> float64
    let b = Rc::clone(&bridge);
    vm.register_fn("groot.GetSelfRotation", move |_| {
        let id = CURRENT_ENTITY.with(|c| c.get());
        let st = b.borrow();
        let state = st.entity_states.get(&EntityId(id)).cloned().unwrap_or_default();
        Value::Float(state.rotation)
    });

    // groot.SetSelfRotation(angle)
    let b = Rc::clone(&bridge);
    vm.register_fn("groot.SetSelfRotation", move |args| {
        let id = CURRENT_ENTITY.with(|c| c.get());
        let rot = args.get(0).and_then(|v| v.as_number()).unwrap_or(0.0);
        let mut st = b.borrow_mut();
        let state = st.entity_states.entry(EntityId(id)).or_default();
        state.rotation = rot;
        Value::Nil
    });

    // groot.GetSelfScale() -> [sx, sy]
    let b = Rc::clone(&bridge);
    vm.register_fn("groot.GetSelfScale", move |_| {
        let id = CURRENT_ENTITY.with(|c| c.get());
        let st = b.borrow();
        let state = st.entity_states.get(&EntityId(id)).cloned().unwrap_or_default();
        Value::Slice(Rc::new(RefCell::new(vec![
            Value::Float(state.scale_x),
            Value::Float(state.scale_y),
        ])))
    });

    // groot.SetSelfScale(sx, sy)
    let b = Rc::clone(&bridge);
    vm.register_fn("groot.SetSelfScale", move |args| {
        let id = CURRENT_ENTITY.with(|c| c.get());
        let sx = args.get(0).and_then(|v| v.as_number()).unwrap_or(1.0);
        let sy = args.get(1).and_then(|v| v.as_number()).unwrap_or(1.0);
        let mut st = b.borrow_mut();
        let state = st.entity_states.entry(EntityId(id)).or_default();
        state.scale_x = sx;
        state.scale_y = sy;
        Value::Nil
    });

    // groot.SetSelfColor(r, g, b, a)
    let b = Rc::clone(&bridge);
    vm.register_fn("groot.SetSelfColor", move |args| {
        let id = CURRENT_ENTITY.with(|c| c.get());
        let r = args.get(0).and_then(|v| v.as_number()).unwrap_or(1.0);
        let g = args.get(1).and_then(|v| v.as_number()).unwrap_or(1.0);
        let b_val = args.get(2).and_then(|v| v.as_number()).unwrap_or(1.0);
        let a = args.get(3).and_then(|v| v.as_number()).unwrap_or(1.0);
        let mut st = b.borrow_mut();
        let state = st.entity_states.entry(EntityId(id)).or_default();
        state.color = (r, g, b_val, a);
        Value::Nil
    });

    // groot.DestroySelf()
    let b = Rc::clone(&bridge);
    vm.register_fn("groot.DestroySelf", move |_| {
        let id = CURRENT_ENTITY.with(|c| c.get());
        let mut st = b.borrow_mut();
        if let Some(state) = st.entity_states.get_mut(&EntityId(id)) {
            state.destroy_requested = true;
        }
        Value::Nil
    });

    // ==================================================================
    // 2. Entity Queries — read other entities' state.
    // ==================================================================

    // groot.GetEntityPosition(id) -> [x, y]
    let b = Rc::clone(&bridge);
    vm.register_fn("groot.GetEntityPosition", move |args| {
        let target_id = args.first().and_then(|v| v.as_number()).unwrap_or(0.0) as u64;
        let st = b.borrow();
        let state = st.entity_states.get(&EntityId(target_id)).cloned().unwrap_or_default();
        Value::Slice(Rc::new(RefCell::new(vec![
            Value::Float(state.x),
            Value::Float(state.y),
        ])))
    });

    // groot.GetDistance(id1, id2) -> float64
    let b = Rc::clone(&bridge);
    vm.register_fn("groot.GetDistance", move |args| {
        let id1 = args.get(0).and_then(|v| v.as_number()).unwrap_or(0.0) as u64;
        let id2 = args.get(1).and_then(|v| v.as_number()).unwrap_or(0.0) as u64;
        let st = b.borrow();
        let s1 = st.entity_states.get(&EntityId(id1)).cloned().unwrap_or_default();
        let s2 = st.entity_states.get(&EntityId(id2)).cloned().unwrap_or_default();
        let dx = s2.x - s1.x;
        let dy = s2.y - s1.y;
        Value::Float((dx * dx + dy * dy).sqrt())
    });

    // ==================================================================
    // 3. Input
    // ==================================================================

    let b = Rc::clone(&bridge);
    vm.register_fn("groot.GetAxis", move |args| {
        let axis = args.first().and_then(|v| v.as_string()).unwrap_or("");
        let st = b.borrow();
        let mut value = 0.0;
        for key in &st.input.keys_down {
            match axis {
                "Horizontal" => {
                    if key == "a" || key == "Left" || key == "KeyA" {
                        value -= 1.0;
                    }
                    if key == "d" || key == "Right" || key == "KeyD" {
                        value += 1.0;
                    }
                }
                "Vertical" => {
                    if key == "w" || key == "Up" || key == "KeyW" {
                        value -= 1.0;
                    }
                    if key == "s" || key == "Down" || key == "KeyS" {
                        value += 1.0;
                    }
                }
                _ => {}
            }
        }
        Value::Float(value)
    });

    let b = Rc::clone(&bridge);
    vm.register_fn("groot.IsKeyPressed", move |args| {
        let key = args.first().and_then(|v| v.as_string()).unwrap_or("");
        let st = b.borrow();
        Value::Bool(st.input.keys_pressed.iter().any(|k| k == key))
    });

    let b = Rc::clone(&bridge);
    vm.register_fn("groot.IsKeyDown", move |args| {
        let key = args.first().and_then(|v| v.as_string()).unwrap_or("");
        let st = b.borrow();
        Value::Bool(st.input.keys_down.iter().any(|k| k == key))
    });

    let b = Rc::clone(&bridge);
    vm.register_fn("groot.IsKeyReleased", move |args| {
        let key = args.first().and_then(|v| v.as_string()).unwrap_or("");
        let st = b.borrow();
        Value::Bool(st.input.keys_released.iter().any(|k| k == key))
    });

    let b = Rc::clone(&bridge);
    vm.register_fn("groot.GetMousePosition", move |_| {
        let st = b.borrow();
        Value::Slice(Rc::new(RefCell::new(vec![
            Value::Float(st.input.mouse_x),
            Value::Float(st.input.mouse_y),
        ])))
    });

    let b = Rc::clone(&bridge);
    vm.register_fn("groot.IsMouseButtonDown", move |args| {
        let btn = args.first().and_then(|v| v.as_number()).unwrap_or(0.0) as usize;
        let st = b.borrow();
        Value::Bool(st.input.mouse_button_down[btn.min(2)])
    });

    let b = Rc::clone(&bridge);
    vm.register_fn("groot.IsMouseButtonPressed", move |args| {
        let btn = args.first().and_then(|v| v.as_number()).unwrap_or(0.0) as usize;
        let st = b.borrow();
        Value::Bool(st.input.mouse_button_pressed[btn.min(2)])
    });

    // ==================================================================
    // 4. Debug Drawing
    // ==================================================================

    let b = Rc::clone(&bridge);
    vm.register_fn("groot.DrawDebugLine", move |args| {
        if args.len() >= 4 {
            let x1 = args[0].as_number().unwrap_or(0.0);
            let y1 = args[1].as_number().unwrap_or(0.0);
            let x2 = args[2].as_number().unwrap_or(0.0);
            let y2 = args[3].as_number().unwrap_or(0.0);
            let r = args.get(4).and_then(|v| v.as_number()).unwrap_or(0.0);
            let g = args.get(5).and_then(|v| v.as_number()).unwrap_or(1.0);
            let b_val = args.get(6).and_then(|v| v.as_number()).unwrap_or(0.0);
            b.borrow_mut().debug_draws.push(DebugDrawCommand::Line {
                x1, y1, x2, y2, r, g, b: b_val,
            });
        }
        Value::Nil
    });

    let b = Rc::clone(&bridge);
    vm.register_fn("groot.DrawDebugCircle", move |args| {
        if args.len() >= 3 {
            let cx = args[0].as_number().unwrap_or(0.0);
            let cy = args[1].as_number().unwrap_or(0.0);
            let radius = args[2].as_number().unwrap_or(10.0);
            let r = args.get(3).and_then(|v| v.as_number()).unwrap_or(0.0);
            let g = args.get(4).and_then(|v| v.as_number()).unwrap_or(1.0);
            let b_val = args.get(5).and_then(|v| v.as_number()).unwrap_or(0.0);
            b.borrow_mut().debug_draws.push(DebugDrawCommand::Circle {
                cx, cy, radius, r, g, b: b_val,
            });
        }
        Value::Nil
    });

    let b = Rc::clone(&bridge);
    vm.register_fn("groot.DrawDebugRect", move |args| {
        if args.len() >= 4 {
            let cx = args[0].as_number().unwrap_or(0.0);
            let cy = args[1].as_number().unwrap_or(0.0);
            let w = args[2].as_number().unwrap_or(10.0);
            let h = args[3].as_number().unwrap_or(10.0);
            let r = args.get(4).and_then(|v| v.as_number()).unwrap_or(1.0);
            let g = args.get(5).and_then(|v| v.as_number()).unwrap_or(1.0);
            let b_val = args.get(6).and_then(|v| v.as_number()).unwrap_or(0.0);
            b.borrow_mut().debug_draws.push(DebugDrawCommand::Rect {
                cx, cy, w, h, r, g, b: b_val,
            });
        }
        Value::Nil
    });

    // ==================================================================
    // 5. Commands
    // ==================================================================

    let b = Rc::clone(&bridge);
    vm.register_fn("groot.SpawnEntity", move |args| {
        let script = args
            .first()
            .and_then(|v| v.as_string())
            .unwrap_or("")
            .to_string();
        let x = args.get(1).and_then(|v| v.as_number()).unwrap_or(0.0);
        let y = args.get(2).and_then(|v| v.as_number()).unwrap_or(0.0);
        b.borrow_mut()
            .commands
            .push(EngineCommand::SpawnEntity { script, x, y });
        Value::Nil
    });

    let b = Rc::clone(&bridge);
    vm.register_fn("groot.PlaySound", move |args| {
        let name = args
            .first()
            .and_then(|v| v.as_string())
            .unwrap_or("")
            .to_string();
        b.borrow_mut()
            .commands
            .push(EngineCommand::PlaySound { name });
        Value::Nil
    });

    // ==================================================================
    // 6. Event Bus
    // ==================================================================

    let b = Rc::clone(&bridge);
    vm.register_fn("groot.EmitEvent", move |args| {
        let name = args.first().and_then(|v| v.as_string()).unwrap_or("");
        let payload = args.get(1).and_then(|v| v.as_number()).unwrap_or(0.0);
        b.borrow_mut().events.push(ScriptEvent {
            event_name: name.to_string(),
            payload,
        });
        Value::Nil
    });
}

// ====================================================================
// Systems — called from the main loop each tick.
// ====================================================================

/// Runs hot-reload + OnUpdate for every entity script.
pub fn system_run_scripts(
    host: &mut GrootScriptHost,
    bridge: &Rc<RefCell<ScriptBridgeState>>,
    dt: f64,
) {
    // Collect entity IDs and scripts to avoid borrow conflicts.
    let entities: Vec<_> = {
        let st = bridge.borrow();
        st.entity_scripts
            .iter()
            .map(|(&eid, path)| (eid, path.clone()))
            .collect()
    };

    for (eid, script_path) in entities {
        let engine = host.ensure_engine_loaded(&script_path, Rc::clone(bridge));

        // Hot-reload: recompile if the .go file changed on disk.
        match engine.reload_if_changed() {
            Ok(true) => {
                println!("[groot] script reloaded: {}", script_path);
            }
            Ok(false) => {}
            Err(e) => {
                eprintln!("[groot] reload error for '{}': {}", script_path, e);
            }
        }

        // Set this entity as the current context.
        CURRENT_ENTITY.with(|c| c.set(eid.0));

        // Set frame delta so time.Delta() works inside scripts.
        engine.vm.set_delta_time(dt);

        // Call OnUpdate(dt) if the script defines it.
        if let Err(e) = engine.vm.call("OnUpdate", vec![Value::Float(dt)]) {
            eprintln!("[groot] entity {:?} OnUpdate error: {}", eid, e);
        }
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
                st.entity_states
                    .insert(new_id, EntityState { x, y, ..Default::default() });
                st.entity_scripts.insert(new_id, script.clone());
                drop(st);
                host.ensure_engine_loaded(&script, Rc::clone(bridge));
                println!("[groot] spawned entity {:?} from '{}'", new_id, script);
            }
            EngineCommand::PlaySound { name } => {
                println!("[groot] PlaySound: {}", name);
            }
        }
    }
}

/// Applies destroy requests from scripts (entities that called DestroySelf).
pub fn system_apply_destroy(bridge: &Rc<RefCell<ScriptBridgeState>>) {
    let to_destroy: Vec<_> = {
        let st = bridge.borrow();
        st.entity_states
            .iter()
            .filter(|(_, s)| s.destroy_requested)
            .map(|(&eid, _)| eid)
            .collect()
    };

    for eid in to_destroy {
        bridge.borrow_mut().entity_states.remove(&eid);
        bridge.borrow_mut().entity_scripts.remove(&eid);
        println!("[groot] entity {:?} destroyed via DestroySelf", eid);
    }
}

/// Processes debug draw commands accumulated during the current tick.
pub fn system_process_debug_draws(bridge: &Rc<RefCell<ScriptBridgeState>>) {
    let draws: Vec<DebugDrawCommand> = bridge.borrow_mut().debug_draws.drain(..).collect();

    for cmd in draws {
        match cmd {
            DebugDrawCommand::Line { x1, y1, x2, y2, r, g, b } => {
                println!(
                    "[groot Gizmo] LINE ({:.1},{:.1})->({:.1},{:.1}) color=({:.2},{:.2},{:.2})",
                    x1, y1, x2, y2, r, g, b
                );
            }
            DebugDrawCommand::Circle { cx, cy, radius, r, g, b } => {
                println!(
                    "[groot Gizmo] CIRCLE ({:.1},{:.1}) r={:.1} color=({:.2},{:.2},{:.2})",
                    cx, cy, radius, r, g, b
                );
            }
            DebugDrawCommand::Rect { cx, cy, w, h, r, g, b } => {
                println!(
                    "[groot Gizmo] RECT ({:.1},{:.1}) {:.1}x{:.1} color=({:.2},{:.2},{:.2})",
                    cx, cy, w, h, r, g, b
                );
            }
        }
    }
}

/// Processes events emitted by scripts during the current tick.
pub fn system_process_events(bridge: &Rc<RefCell<ScriptBridgeState>>) {
    let events: Vec<ScriptEvent> = bridge.borrow_mut().events.drain(..).collect();

    for ev in events {
        println!(
            "[groot EventBus] '{}' payload={:.2}",
            ev.event_name, ev.payload
        );
    }
}
