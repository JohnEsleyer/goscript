mod bridge;
mod module;
mod plugin;

use std::cell::RefCell;
use std::rc::Rc;

use bridge::{GrootScriptHost, InputState, ScriptBridgeState};

fn main() {
    println!("=== GoScript Multi-Entity Runtime Demo ===");
    println!();

    // Shared world state.
    let bridge = Rc::new(RefCell::new(ScriptBridgeState::new()));
    let mut host = GrootScriptHost::new();

    // ------------------------------------------------------------------
    // Scene setup — spawn entities at initial positions.
    // ------------------------------------------------------------------
    {
        let mut st = bridge.borrow_mut();

        // Player
        let player_id = st.spawn_id();
        st.positions.insert(player_id, (400.0, 300.0));
        st.entity_scripts
            .insert(player_id, "scripts/player.gs".to_string());

        // Enemy
        let enemy_id = st.spawn_id();
        st.positions.insert(enemy_id, (200.0, 150.0));
        st.entity_scripts
            .insert(enemy_id, "scripts/enemy.gs".to_string());

        // NPC
        let npc_id = st.spawn_id();
        st.positions.insert(npc_id, (600.0, 400.0));
        st.entity_scripts
            .insert(npc_id, "scripts/npc.gs".to_string());

        // Store IDs so we can reference them later.
        st.entity_scripts
            .insert(player_id, "scripts/player.gs".to_string());
        println!(
            "[setup] player={:?} enemy={:?} npc={:?}",
            player_id, enemy_id, npc_id
        );
    }

    // Spawn each entity's script engine.
    {
        let st = bridge.borrow();
        let entries: Vec<_> = st
            .entity_scripts
            .iter()
            .map(|(&eid, path)| (eid, path.clone()))
            .collect();
        drop(st);
        for (eid, path) in entries {
            host.spawn_entity(eid, &path, Rc::clone(&bridge));
        }
    }

    // ------------------------------------------------------------------
    // Simulated game loop — 5 ticks with fake input.
    // ------------------------------------------------------------------
    println!();
    println!("--- tick 1: player moves right ---");
    simulate_tick(&mut host, &bridge, 0.016, |st| {
        st.input.keys_down.push("d".to_string());
    });

    println!();
    println!("--- tick 2: player moves down, enemy idles ---");
    simulate_tick(&mut host, &bridge, 0.016, |st| {
        st.input.keys_down.push("s".to_string());
    });

    println!();
    println!("--- tick 3: player attacks (space) ---");
    simulate_tick(&mut host, &bridge, 0.016, |st| {
        st.input.keys_pressed.push("Space".to_string());
    });

    println!();
    println!("--- tick 4: no input ---");
    simulate_tick(&mut host, &bridge, 0.016, |_| {});

    println!();
    println!("--- tick 5: spawn a new enemy via script command ---");
    simulate_tick(&mut host, &bridge, 0.016, |_| {});

    // ------------------------------------------------------------------
    // Print final state.
    // ------------------------------------------------------------------
    println!();
    println!("=== Final Entity Positions ===");
    {
        let st = bridge.borrow();
        for (eid, pos) in &st.positions {
            println!("  entity {:?}: ({:.1}, {:.1})", eid, pos.0, pos.1);
        }
    }

    println!();
    println!("=== Done ===");
}

/// Runs one tick: sync input -> run scripts -> process commands.
fn simulate_tick(
    host: &mut GrootScriptHost,
    bridge: &Rc<RefCell<ScriptBridgeState>>,
    dt: f64,
    input_setup: impl FnOnce(&mut ScriptBridgeState),
) {
    // 1. Set up this tick's input.
    input_setup(&mut bridge.borrow_mut());

    // 2. Sync input (placeholder for future global injection).
    plugin::system_sync_input(host, bridge);

    // 3. Run all entity scripts.
    plugin::system_run_scripts(host, bridge, dt);

    // 4. Process deferred commands.
    plugin::system_process_commands(host, bridge);

    // 5. Clear per-frame input.
    bridge.borrow_mut().input = InputState::default();
}
