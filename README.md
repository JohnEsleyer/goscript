# GoScript

**An embeddable scripting language with Go syntax, written in pure Rust, designed for game engines.**

Go is beloved for its clean, readable syntax. But the Go runtime — its virtual
machine, scheduler, and garbage collector — is a poor fit for games: stop-the-world
GC pauses, background goroutines, large binaries, and a painful FFI story. GoScript
keeps the look and feel of Go and throws the runtime away.

```go
// player.gs — the syntax you already know and love
var hp = 100
var speed = 12.5

func OnUpdate(dt float64) {
    if hp > 0 {
        MovePosition(GetAxis("Horizontal")*speed*dt, GetAxis("Vertical")*speed*dt)
    } else {
        DestroyEntity()
    }
}

func TakeDamage(amount int) {
    hp = hp - amount
    Log("Player took damage, remaining HP:", hp)
}
```

## Why GoScript?

| Problem | Solution |
| --- | --- |
| Stop-the-world GC pauses | No garbage collector; values are reference-counted and scoped. |
| Go runtime background threads | Nothing runs on its own — the engine drives execution tick by tick. |
| Huge binaries + CGO wrapper pain | A pure-Rust VM, compiled directly into your game. |
| Go's static compiler | A tiny `lexer → parser → bytecode compiler → stack VM`. |

### Principle

> *Your game engine lives mainly in Rust but you want your content team
> to write gameplay scripts without learning Rust. They already know Go. So let
> them write Go — the syntax — and let the engine own the runtime.*

## Features

Fast, dependency-free core:

- **Go-flavored syntax** — `var`, `func`, `if`/`else`, `for`, `type struct`,
  typed parameters and `:=` short declarations, `//` and `/* */` comments,
  automatic semicolon insertion (ASI) so semicolons are optional.
- **Dynamically typed values** (optional type annotations accepted and ignored) —
  `nil`, `int` (i64), `float` (f64), `string`, `bool`.
- **Structs** — `type Player struct { hp int }`, struct literals
  `Player{hp: 100}`, and shared references: assignments alias the same instance
  (backed by `Rc<RefCell>`), so mutations show through every alias.
- **Bytecode VM** — the front end emits a chunk-based instruction set (mirrors
  Lua's design), executed by a fast stack VM with global + local slots.
- **Functions & recursion** with `return`, `break`, `continue`.
- **Hot reloading** — recompile `.gs` files on change while preserving live
  global state across reloads.
- **Native standard library** — Go's heavy stdlib (`net`, `os`, `exec`, `db`) is
  left out on purpose; a tiny game-focused `math`, `fmt`, `rand`, `time` is
  registered as compiled-in Rust functions and called with zero script
  overhead. Sandboxed by construction: no filesystem, network, or process API.
- **Host bindings** — `register_fn` exposes Rust functions to scripts.
- **No background execution** — scripts only run when *you* call them.
- Clean modules: `lexer`, `parser`, `ast`, `compiler`, `opcode`, `function`,
  `vm`, `value`, `hot_reload`.

## Quick start

```bash
cargo build
cargo run                              # hot-reload + stdlib frame-tick demo
cargo run -- examples/player.gs           # run a standalone script file
cargo run -- examples/struct_demo.gs
cargo run -- examples/stdlib_demo.gs
cargo test
```

## Embedding in your engine

```rust
use goscript::value::Value;
use goscript::vm::VirtualMachine;

let mut vm = VirtualMachine::new();   // standard library auto-registered
vm.register_fn("Log", |args| {
    println!("{}", args.iter().map(|a| a.to_string()).collect::<Vec<_>>().join(" "));
    Value::Nil
});

let chunk = vm.compile("
    var hp = 100
    func TakeDamage(amount int) int {
        hp = hp - amount
        return hp
    }
")?;
vm.execute(chunk)?;

// each frame, from your main loop:
let hp = vm.call("TakeDamage", vec![Value::Int(35)])?;  // Value::Int(65)
```

Hot reloading, from a host that owns the script file:

```rust
let mut engine = HotReloadEngine::new("content/player.gs");
// every tick, cheaply:
let changed = engine.reload_if_changed()?;  // recompiles only when the file changes
engine.vm.call("OnUpdate", vec![Value::Float(dt)])?;
```

## Standard library

Sandboxed: no `os`, no `net`, no `exec` — nothing a third-party mod could use
to touch the machine. Instead, the curated set below, each function a plain
Rust call (native speed, zero script overhead).

```go
func CalculateDamage(distance float64) float64 {
    var falloff = math.Clamp(1.0 / math.Sqrt(distance), 0.1, 1.0)
    var rawDamage = 50.0 * falloff
    fmt.Println("Calculated damage falloff:", falloff)
    if rand.Float() > 0.5 {
        fmt.Println("Crit!")
    }
    return rawDamage
}
```

`time.Delta()` returns the frame's delta set by the host via
`vm.set_delta_time(dt)` (call it once per frame before ticking scripts);
`time.Now()` is a monotonic engine-run-time clock.

| Package | Functions |
| --- | --- |
| `math` | `Abs`, `Sqrt`, `Sin`, `Cos`, `Min`, `Max`, `Clamp`, `Floor`, `Ceil`, `Round`, `Atan2`, `Pow`, `Pi` |
| `fmt` | `Println(...)`, `Sprintf(fmt, ...)` — `%d`, `%f`, `%s`, `%v`, `%%` |
| `rand` | `Float()` (\[0, 1)), `Intn(n)` (\[0, n)) — fast xorshift, seeded from the clock |
| `time` | `Delta()` (frame dt from `set_delta_time`), `Now()` (run-time seconds) |
| `strings` | `Contains`, `ToLower`, `ToUpper`, `HasPrefix`, `HasSuffix`, `Trim`, `Replace`, `Split`, `Join` |

```rust
let mut vm = VirtualMachine::new();   // stdlib auto-registered
vm.register_fn("Log", |args| { /* host log */ Value::Nil });
vm.set_delta_time(0.016);             // feed frame dt to time.Delta()
```

## Features at a glance

| Construct | Example |
| --- | --- |
| Variables | `var hp = 100`, `var speed float64 = 2.5`, `x := 1` |
| Assignment | `hp = hp - amount` |
| Conditionals | `if hp > 0 { } else { }` |
| Loops | `for i := 0; i < n; i = i + 1 { }`, `for cond { }`, `for { }` |
| Loop control | `break`, `continue` |
| Functions | `func Add(a int, b int) int { return a + b }` |
| Structs | `type Transform struct { X, Y float64 }` + `Transform{X: 0, Y: 0}` |
| Field access | `player.hp = player.hp - 25`, `transform.X` |
| Packages | `math.Sqrt(x)`, `fmt.Sprintf(...)`, `rand.Intn(n)`, `time.Delta()` |
| Types | `nil`, ints, floats, strings, bools, structs |
| Operators | arithmetic, comparison, `&&` / `\|\|` / `!`, string concatenation with `+` |
| Host bridge | `register_fn(name, fn(Vec<Value>) -> Value)` |
| Hot reload | `HotReloadEngine::reload_if_changed()` preserves live globals |

## Intent

GoScript is **not** a Go-compatible compiler or a general-purpose language. It
is a purpose-built *game-dev scripting dialect*: the syntax ergonomics of Go,
stripped of anything that gets in the way of embedding — no goroutines, no
channels, no `panic`/`defer`, no interfaces, no GC, no standard library. Scripts
are data: plain text that the engine compiles, loads, calls, and hot-reloads
within a tick of the main loop.

## Roadmap

Done: bytecode VM, structs (reference semantics), hot reloading, native
standard library (`math`, `fmt`, `rand`, `time`).

### Todo

- [x] `strings` stdlib — `Contains`, `ToLower`, `ToUpper`, `HasPrefix`, `HasSuffix`, `Trim`, `Replace`, `Split`, `Join`
- [x] Extend `math` — `Floor`, `Ceil`, `Round`, `Atan2`, `Pow`
- [x] Recursive hot reloading — `resolve_imports_with_deps` returns dependency set; `HotReloadEngine` watches all imports
- [x] `CallMethod` host support — VM checks `host_fns["TypeName.Method"]` before globals for receiver-style calls
- [x] Multi-entity architecture — per-entity VM instances via `GrootScriptHost`, `ScriptBridgeState` for shared game state, command queue for deferred ops
- [x] `GrootModuleExt` rewrite — bridge-based `Input`, `Entity`, `Position`, `Spawn`, `Destroy` APIs
- [x] `groot_plugin.rs` rewrite — sync-input, process-commands, run-scripts systems
- [x] Scene setup in `main.rs` — multi-entity demo with player, enemy, NPC, camera
- [x] Script assets — `player.gs`, `enemy.gs`, `npc.gs`, `utils/math_helpers.gs`

Still planned:

- **Methods** — `entity.Move(dx, dy)` sugar over host bindings.
- **Closures** and first-class function values.
- **Arrays/slices**, `range`, and a small standard library.

## License

Licensed under either of Apache-2.0 or MIT at your option.
