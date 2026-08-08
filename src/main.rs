use std::env;
use std::fs;
use std::path::PathBuf;

use goscript::value::Value;
use goscript::vm::VirtualMachine;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().skip(1).collect();

    if let Some(path) = args.first() {
        run_script_file(path)?;
        return Ok(());
    }

    run_engine_demo()
}

fn build_vm() -> VirtualMachine {
    let mut vm = VirtualMachine::new();
    vm.register_fn("Log", |args| {
        let msg: Vec<String> = args.iter().map(|a| a.to_string()).collect();
        println!("{}", msg.join(" "));
        Value::Nil
    });
    vm
}

fn run_script_file(path: &str) -> Result<(), Box<dyn std::error::Error>> {
    let source = fs::read_to_string(path)?;
    let mut vm = build_vm();
    let chunk = vm.compile(&source)?;
    vm.execute(chunk)?;
    Ok(())
}

/// Engine-style demo: hot-reload a `.gs` file and drive OnUpdate() frame by
/// frame. Frame delta is pushed into the VM (`time.Delta()`) and the script
/// freely uses the native `math`, `fmt`, `rand`, and `time` packages.
fn run_engine_demo() -> Result<(), Box<dyn std::error::Error>> {
    let script_path = PathBuf::from(env::temp_dir()).join("goscript_actor.gs");
    let demo = fs::read_to_string("examples/stdlib_demo.gs")?;
    fs::write(&script_path, demo)?;

    let mut engine = goscript::HotReloadEngine::new(script_path.to_str().unwrap());
    engine.reload_if_changed()?;

    println!("\n--- Frame ticks with the GoScript standard library ---");
    for frame in 0..3 {
        engine.vm.set_delta_time(0.016);
        engine.vm.call("OnUpdate", vec![Value::Float(0.016)])?;
        println!("  [frame {frame}] MaxSpeed = {}", engine.vm.call("MaxSpeed", vec![])?);
    }

    fs::remove_file(&script_path)?;
    Ok(())
}