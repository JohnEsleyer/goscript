use std::cell::RefCell;
use std::rc::Rc;

use goscript::value::Value;
use goscript::vm::VirtualMachine;

use crate::bridge::{EntityId, EngineCommand, ScriptBridgeState};

/// Registers the full GoScript ↔ Groot API on a per-entity VM.
///
/// Every function reads/writes the shared [`ScriptBridgeState`] through the
/// captured `Rc<RefCell<...>>`, so each entity sees the same game world.
pub fn register_groot_api(
    vm: &mut VirtualMachine,
    entity_id: EntityId,
    bridge: Rc<RefCell<ScriptBridgeState>>,
) {
    // ------------------------------------------------------------------
    // Entity info
    // ------------------------------------------------------------------
    let eid = entity_id;
    vm.register_fn("Entity", move |_| Value::Int(eid.0 as i64));

    // ------------------------------------------------------------------
    // Position
    // ------------------------------------------------------------------
    let b = Rc::clone(&bridge);
    let eid = entity_id;
    vm.register_fn("GetPosition", move |_| {
        let st = b.borrow();
        let (x, y) = st.positions.get(&eid).copied().unwrap_or((0.0, 0.0));
        let pair = Rc::new(RefCell::new(vec![Value::Float(x), Value::Float(y)]));
        Value::Slice(pair)
    });

    let b = Rc::clone(&bridge);
    let eid = entity_id;
    vm.register_fn("SetPosition", move |args| {
        let x = args.get(0).and_then(|v| v.as_number()).unwrap_or(0.0);
        let y = args.get(1).and_then(|v| v.as_number()).unwrap_or(0.0);
        b.borrow_mut().positions.insert(eid, (x, y));
        Value::Nil
    });

    let b = Rc::clone(&bridge);
    let eid = entity_id;
    vm.register_fn("MovePosition", move |args| {
        let dx = args.get(0).and_then(|v| v.as_number()).unwrap_or(0.0);
        let dy = args.get(1).and_then(|v| v.as_number()).unwrap_or(0.0);
        let mut st = b.borrow_mut();
        let pos = st.positions.entry(eid).or_insert((0.0, 0.0));
        pos.0 += dx;
        pos.1 += dy;
        Value::Nil
    });

    // ------------------------------------------------------------------
    // Input queries
    // ------------------------------------------------------------------
    let b = Rc::clone(&bridge);
    vm.register_fn("GetAxis", move |args| {
        let axis = args.get(0).and_then(|v| v.as_string()).unwrap_or("");
        let st = b.borrow();
        let mut value = 0.0;
        for key in &st.input.keys_down {
            match axis {
                "Horizontal" => {
                    if key == "a" || key == "Left" {
                        value -= 1.0;
                    }
                    if key == "d" || key == "Right" {
                        value += 1.0;
                    }
                }
                "Vertical" => {
                    if key == "w" || key == "Up" {
                        value -= 1.0;
                    }
                    if key == "s" || key == "Down" {
                        value += 1.0;
                    }
                }
                _ => {}
            }
        }
        Value::Float(value)
    });

    let b = Rc::clone(&bridge);
    vm.register_fn("InputKeyPressed", move |args| {
        let key = args.get(0).and_then(|v| v.as_string()).unwrap_or("");
        let st = b.borrow();
        Value::Bool(st.input.keys_pressed.iter().any(|k| k == key))
    });

    let b = Rc::clone(&bridge);
    vm.register_fn("InputKeyDown", move |args| {
        let key = args.get(0).and_then(|v| v.as_string()).unwrap_or("");
        let st = b.borrow();
        Value::Bool(st.input.keys_down.iter().any(|k| k == key))
    });

    let b = Rc::clone(&bridge);
    vm.register_fn("MousePosition", move |_| {
        let st = b.borrow();
        let pair = Rc::new(RefCell::new(vec![
            Value::Float(st.input.mouse_x),
            Value::Float(st.input.mouse_y),
        ]));
        Value::Slice(pair)
    });

    let b = Rc::clone(&bridge);
    vm.register_fn("MousePressed", move |_| {
        let st = b.borrow();
        Value::Bool(st.input.mouse_pressed)
    });

    let b = Rc::clone(&bridge);
    vm.register_fn("DistanceToEntity", move |args| {
        let other_id = args.get(0).and_then(|v| v.as_number()).unwrap_or(0.0) as u64;
        let st = b.borrow();
        let my_pos = st.positions.get(&eid).copied().unwrap_or((0.0, 0.0));
        let other_pos = st
            .positions
            .get(&EntityId(other_id))
            .copied()
            .unwrap_or((0.0, 0.0));
        let dx = my_pos.0 - other_pos.0;
        let dy = my_pos.1 - other_pos.1;
        Value::Float((dx * dx + dy * dy).sqrt())
    });

    // ------------------------------------------------------------------
    // Command queue (deferred)
    // ------------------------------------------------------------------
    let b = Rc::clone(&bridge);
    vm.register_fn("SpawnEntity", move |args| {
        let script = args
            .get(0)
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
    vm.register_fn("DestroyEntity", move |_| {
        b.borrow_mut()
            .commands
            .push(EngineCommand::DestroyEntity(entity_id));
        Value::Nil
    });

    let b = Rc::clone(&bridge);
    vm.register_fn("PlaySound", move |args| {
        let name = args
            .get(0)
            .and_then(|v| v.as_string())
            .unwrap_or("")
            .to_string();
        b.borrow_mut()
            .commands
            .push(EngineCommand::PlaySound { name });
        Value::Nil
    });

    // ------------------------------------------------------------------
    // Utilities
    // ------------------------------------------------------------------
    vm.register_fn("Log", |args| {
        let msg: Vec<String> = args.iter().map(|a| a.to_string()).collect();
        println!("[groot] {}", msg.join(" "));
        Value::Nil
    });
}
