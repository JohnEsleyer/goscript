mod bridge;
mod groot_module;
mod groot_plugin;

use std::cell::RefCell;
use std::rc::Rc;

use bridge::{ EntityState, GrootScriptHost, InputState, ScriptBridgeState};

fn main() {
    println!("=== Groot Engine — Hybrid Component-Behavior Architecture ===");
    println!("   Raylib ergonomics + ECS entity isolation + event bus");
    println!();

    let bridge = Rc::new(RefCell::new(ScriptBridgeState::new()));
    let mut host = GrootScriptHost::new();

    // ------------------------------------------------------------------
    // Scene setup — Player (ID #1) and Enemy (ID #2)
    // ------------------------------------------------------------------
    {
        let mut st = bridge.borrow_mut();

        let player_id = st.spawn_id();
        st.entity_states.insert(
            player_id,
            EntityState {
                x: -100.0,
                y: 0.0,
                rotation: 0.0,
                scale_x: 1.0,
                scale_y: 1.0,
                color: (0.1, 0.8, 0.3, 1.0),
                destroy_requested: false,
            },
        );
        st.entity_scripts
            .insert(player_id, "examples/groot/scripts/player.go".to_string());

        let enemy_id = st.spawn_id();
        st.entity_states.insert(
            enemy_id,
            EntityState {
                x: 100.0,
                y: 0.0,
                rotation: 0.0,
                scale_x: 1.0,
                scale_y: 1.0,
                color: (0.8, 0.2, 0.8, 1.0),
                destroy_requested: false,
            },
        );
        st.entity_scripts
            .insert(enemy_id, "examples/groot/scripts/enemy.go".to_string());

        println!("[setup] player={:?} enemy={:?}", player_id, enemy_id);
    }

    // ------------------------------------------------------------------
    // Game loop — 6 ticks
    // ------------------------------------------------------------------
    println!();
    println!("--- tick 1: player moves right ---");
    simulate_tick(&mut host, &bridge, 0.016, |st| {
        st.input.keys_down.push("KeyD".to_string());
    });

    println!();
    println!("--- tick 2: player moves down ---");
    simulate_tick(&mut host, &bridge, 0.016, |st| {
        st.input.keys_down.push("KeyS".to_string());
    });

    println!();
    println!("--- tick 3: player attacks (Space) ---");
    simulate_tick(&mut host, &bridge, 0.016, |st| {
        st.input.keys_pressed.push("Space".to_string());
    });

    println!();
    println!("--- tick 4: player moves left ---");
    simulate_tick(&mut host, &bridge, 0.016, |st| {
        st.input.keys_down.push("KeyA".to_string());
    });

    println!();
    println!("--- tick 5: no input ---");
    simulate_tick(&mut host, &bridge, 0.016, |_| {});

    println!();
    println!("--- tick 6: mouse click near player ---");
    simulate_tick(&mut host, &bridge, 0.016, |st| {
        st.input.mouse_x = -80.0;
        st.input.mouse_y = 10.0;
        st.input.mouse_button_down[0] = true;
        st.input.mouse_button_pressed[0] = true;
    });

    // ------------------------------------------------------------------
    // Final state
    // ------------------------------------------------------------------
    println!();
    println!("=== Final Entity State ===");
    {
        let st = bridge.borrow();
        for (eid, state) in &st.entity_states {
            let default_script = String::new();
            let script = st.entity_scripts.get(eid).unwrap_or(&default_script);
            println!(
                "  {:?} [{}]: pos=({:.1},{:.1}) rot={:.2} scale=({:.1},{:.1}) color=({:.2},{:.2},{:.2},{:.2})",
                eid, script, state.x, state.y, state.rotation,
                state.scale_x, state.scale_y,
                state.color.0, state.color.1, state.color.2, state.color.3
            );
        }
    }

    println!();
    println!("=== Done ===");
}

fn simulate_tick(
    host: &mut GrootScriptHost,
    bridge: &Rc<RefCell<ScriptBridgeState>>,
    dt: f64,
    input_setup: impl FnOnce(&mut ScriptBridgeState),
) {
    input_setup(&mut bridge.borrow_mut());
    groot_plugin::system_run_scripts(host, bridge, dt);
    groot_plugin::system_process_commands(host, bridge);
    groot_plugin::system_apply_destroy(bridge);
    groot_plugin::system_process_debug_draws(bridge);
    groot_plugin::system_process_events(bridge);
    bridge.borrow_mut().input = InputState::default();
}
