```text
================================================================================
                    GOSCRIPT COMPREHENSIVE DEVELOPER & LLM MANUAL
================================================================================

TABLE OF CONTENTS
-----------------
1. Overview & Core Philosophy
2. Language Syntax & Dialect Guide
3. Value System & Memory Model
4. Standard Library Reference
5. Module & Import System
6. Bytecode VM & Compiler Architecture
7. Host Rust API Reference
8. Receiver Methods & CallMethod Mechanics
9. Hot Reloading Architecture
10. Engine Integration Patterns (e.g., Bevy / Groot)
11. Common Gotchas & Architectural Rules for LLMs/Contributors

================================================================================
1. OVERVIEW & CORE PHILOSOPHY
================================================================================

GoScript is a pure-Rust embeddable scripting engine that compiles a Go-flavored
dialect into a custom bytecode chunk format and executes it on a fast stack-based 
virtual machine.

Designed specifically for game engines:
- NO Garbage Collector: Primitives are stack/register values; compound types 
  (Structs, Slices, Maps) are reference-counted (`Rc<RefCell<...>>`) and scoped.
- NO Background Execution or Goroutines: Scripts are strictly deterministic data;
  execution only occurs when explicitly called frame-by-frame by the host engine.
- Zero Heavy Dependencies: Pure Rust, tiny footprint, sandbox-safe by construction.
- Hot Reloading: Live state (global variables) persists across recompilations.
- Standard `.go` Extensions: Developers write scripts in `.go` files, enabling 
  immediate syntax highlighting and IDE tooling out of the box.

Pipeline Architecture:
  Source Code (.go) 
    --> Lexer (Tokens) 
    --> Parser (AST) 
    --> Resolver (Imports & Package Qualification) 
    --> Compiler (Bytecode & Constants) 
    --> Virtual Machine Execution

================================================================================
2. LANGUAGE SYNTAX & DIALECT GUIDE
================================================================================

GoScript matches Go's visual syntax ergonomics while stripping heavy runtime 
features (channels, interfaces, defer/panic, goroutines).

Variables & Assignments:
  var hp int = 100
  var speed = 12.5          // Type annotation is accepted and ignored
  x := 10                   // Short declaration sugar (desugars to var x = 10)
  hp = hp - 15              // Assignment
  count++                   // Increment (desugars to count = count + 1)
  total += 5                // Compound assignment (+=, -=, *=, /=)

Conditionals & Control Flow:
  if hp > 0 {
      // ...
  } else if hp == 0 {
      // ...
  } else {
      // ...
  }

Switch Statements:
  switch state {
  case "IDLE":
      label = "waiting"
  case "ATTACK":
      label = "burning"
  default:
      label = "lost"
  }

Loops:
  for i := 0; i < 10; i++ {           // Standard C-style for loop
      if i == 2 { continue }
      if i == 8 { break }
  }
  for hp > 0 { }                      // While-style condition loop
  for { }                             // Infinite loop (guarded by VM instruction budget)
  for i, v := range slice { }         // Range loop over slice (desugared at parse time)
  for i := range slice { }            // Range loop with index only

Functions & Receiver Methods:
  func Add(a int, b int) int {
      return a + b
  }

  type Player struct {
      Hp int
  }

  // Receiver method syntax:
  func (p *Player) TakeDamage(amount int) {
      p.Hp -= amount
  }

Structs & Compound Literals:
  type Transform struct {
      X float64
      Y float64
  }

  var pos = Transform{X: 10.0, Y: 20.0}
  var nums = []int{1, 2, 3}                           // Slices
  var inv = map[string]int{"potions": 3, "gold": 100} // Maps

Semicolons:
  Automatic Semicolon Insertion (ASI) is active in the Lexer. Semicolons are 
  optional at line endings.

================================================================================
3. VALUE SYSTEM & MEMORY MODEL
================================================================================

Rust Enum Definition (`goscript::value::Value`):
------------------------------------------------
pub enum Value {
    Nil,
    Int(i64),
    Float(f64),
    String(String),
    Bool(bool),
    Struct(Rc<RefCell<StructInstance>>),
    Slice(Rc<RefCell<Vec<Value>>>),
    Map(Rc<RefCell<HashMap<String, Value>>>),
    Function(Rc<CompiledFunction>),
    NativeFn(fn(Vec<Value>) -> Value),
}

Memory Semantics:
- Primitives (`Nil`, `Int`, `Float`, `String`, `Bool`) are value types. Copying them 
  creates a new value.
- Compound Types (`Struct`, `Slice`, `Map`) use reference semantics wrapped in
  `Rc<RefCell<T>>`.
- Aliasing: Assigning a struct/slice variable to another variable shares the 
  underlying instance (`Rc::clone`). Mutations through one alias are immediately 
  visible to all other aliases.
- Equality (`==`): 
  - Primitives: Value equality. Int and Float comparisons cross-coerce (`10 == 10.0` is true).
  - Structs / Slices / Maps: Reference pointer equality (`Rc::ptr_eq`).

`Value` Helper Methods:
- `val.truthy() -> bool`: Returns false for `Nil`, `false`, `0`, `0.0`, `""`; true otherwise.
- `val.as_number() -> Option<f64>`: Coerces `Int(i64)` or `Float(f64)` to `f64`.
- `val.as_string() -> Option<&str>`: Extracts `&str` if variant is `String`.
- `val.to_string()`: Implements `Display`. Produces clean formatted representations.

Struct Representation (`goscript::value::StructInstance`):
  pub struct StructInstance {
      pub type_name: String,
      pub fields: HashMap<String, Value>,
  }

================================================================================
4. STANDARD LIBRARY REFERENCE
================================================================================

All standard library functions are written in native Rust and auto-registered in 
every `VirtualMachine` instance. Calling them carries zero script overhead.

Built-in Functions (Global Scope):
----------------------------------
- `len(coll) -> int`: Returns length of a `Slice`, `Map`, or `String`.
- `append(slice, item) -> slice`: Appends item to slice and returns modified slice.
- `int(v) -> int`: Casts float/string/bool/int to `i64`.
- `float64(v) -> float64`: Casts int/string/float to `f64`.
- `string(v) -> string`: Converts any value to `string`.
- `bool(v) -> bool`: Coerces any value to boolean truthiness.

Package `math`:
---------------
- Constants: `math.Pi`
- Functions: `math.Sqrt(x)`, `math.Abs(x)`, `math.Sin(x)`, `math.Cos(x)`,
  `math.Min(a, b)`, `math.Max(a, b)`, `math.Clamp(v, min, max)`, `math.Floor(x)`,
  `math.Ceil(x)`, `math.Round(x)`, `math.Atan2(y, x)`, `math.Pow(base, exp)`

Package `fmt`:
--------------
- `fmt.Println(...)`: Prints arguments separated by space to stdout.
- `fmt.Sprintf(format_str, ...)`: Formats strings supporting `%d` (int), `%f` (float, 6 decimals),
  `%s` (string), `%v` (any value display), `%%` (literal %).

Package `rand`:
---------------
- `rand.Float() -> float64`: Returns random float in range [0.0, 1.0). Powered by fast internal xorshift64*.
- `rand.Intn(n) -> int`: Returns random integer in range [0, n).

Package `time`:
---------------
- `time.Delta() -> float64`: Returns the current frame delta time set by host via `vm.set_delta_time(dt)`.
- `time.Now() -> float64`: Returns monotonic engine runtime in seconds.

Package `strings`:
------------------
- `strings.Contains(s, substr) -> bool`
- `strings.ToLower(s) -> string`
- `strings.ToUpper(s) -> string`
- `strings.HasPrefix(s, prefix) -> bool`
- `strings.HasSuffix(s, suffix) -> bool`
- `strings.Trim(s, cutset) -> string`
- `strings.Replace(s, old, new, n) -> string` (if n omitted, replaces all)
- `strings.Split(s, sep) -> []string`
- `strings.Join(slice, sep) -> string`

================================================================================
5. MODULE & IMPORT SYSTEM
================================================================================

Syntax:
  import "utils/math_helpers.go"
  import (
      "a.go"
      "b.go"
  )

Resolution Mechanics (`goscript::resolver`):
1. Import paths are resolved using a `ScriptResolver` implementation (e.g., `DiskScriptResolver`
   or `MemoryScriptResolver`).
2. Module Namespacing: Top-level functions and variables in an imported file are automatically 
   namespaced under the file's stem name (`"utils/math_helpers.go"` -> `math_helpers`).
3. Self-Qualification: Internal references within the imported module to its own top-level symbols 
   are rewritten during resolution to `module_name.symbol`.
4. Package Registration: Imported module stems are auto-declared as packages (`declare_package("mod")`)
   so dotted access (`math_helpers.Lerp(...)`) is parsed directly as dotted calls rather than struct field reads.
5. Circular Import Guard: Active import stacks are tracked; circular chains raise a compile error.
6. Receiver Methods (`func (z *Zombie) Hit()`) and Struct Declarations are NOT namespaced. 
   Receiver methods remain `<TypeName>.<Method>`.

================================================================================
6. BYTECODE VM & COMPILER ARCHITECTURE
================================================================================

Data Structures:
- `CompiledFunction`: Holds `name: String`, `arity: usize`, `code: Vec<u8>`, 
  `constants: Vec<Value>`, and `lines: Vec<usize>` (parallel to code array for P1 error reporting).
- `CallFrame`: Holds `function: Rc<CompiledFunction>`, `ip: usize`, `slots_offset: usize`.

Opcodes (`goscript::opcode::OpCode`):
-------------------------------------
Value Loading & Stack:
  Constant(0), Nil(1), True(2), False(3), Pop(17)

Arithmetic & Logic:
  Add(4), Sub(5), Mul(6), Div(7), Mod(8), Negate(9), Not(10)

Comparisons:
  Greater(11), GreaterEqual(12), Less(13), LessEqual(14), Equal(15), NotEqual(16)

Globals & Locals:
  GetGlobal(18), SetGlobal(19), GetLocal(20), SetLocal(21)

Jumps & Control Flow:
  JumpIfFalse(22), JumpIfFalseKeep(23), JumpIfTrueKeep(24), Jump(25)

Functions & Calls:
  Call(26), Return(27), CallMethod(35)

Structs & Collections:
  NewStruct(28), GetField(29), SetField(30), NewSlice(31), NewMap(32), GetIndex(33), SetIndex(34)

Anti-Freeze Guard:
- Every VM instance has a configurable instruction limit (`vm.set_max_instructions(Some(1_000_000))`).
- Infinite loops raise a runtime error carrying the exact script line number instead of freezing the host process.

================================================================================
7. HOST RUST API REFERENCE
================================================================================

Basic Usage:
------------
```rust
use goscript::value::Value;
use goscript::vm::VirtualMachine;

let mut vm = VirtualMachine::new();

// 1. Register Host Native Function (capturing closures supported via Rc<dyn Fn>)
vm.register_fn("groot.Log", |args| {
    println!("{}", args.iter().map(|a| a.to_string()).collect::<Vec<_>>().join(" "));
    Value::Nil
});

// 2. Feed Delta Time per frame
vm.set_delta_time(0.016);

// 3. Compile Script Source
let source = r#"
    var hp = 100
    func TakeDamage(amount int) int {
        hp -= amount
        return hp
    }
"#;
let chunk = vm.compile(source)?;

// 4. Execute Top-Level Statements (Initializes Globals)
vm.execute(chunk)?;

// 5. Invoke Script Function from Host
let remaining_hp = vm.call("TakeDamage", vec![Value::Int(25)])?;
assert_eq!(remaining_hp, Value::Int(75));
```

Virtual Machine Methods:
------------------------
- `VirtualMachine::new() -> VirtualMachine`: Initializes VM with stdlib registered.
- `vm.set_delta_time(dt: f64)`: Pushes frame delta into global atomic for `time.Delta()`.
- `vm.set_max_instructions(max: Option<u64>)`: Sets anti-freeze instruction budget.
- `vm.set_resolver(resolver: Rc<dyn ScriptResolver>)`: Sets custom file resolver for `import`.
- `vm.register_fn(name: &str, f: impl Fn(Vec<Value>) -> Value + 'static)`: Exposes Rust closure to scripts. Auto-declares package if `name` contains `.`.
- `vm.compile(source: &str) -> Result<Rc<CompiledFunction>, Error>`: Lexes, parses, resolves, and compiles script string.
- `vm.execute(chunk: Rc<CompiledFunction>) -> Result<(), Error>`: Executes top-level bytecode.
- `vm.call(name: &str, args: Vec<Value>) -> Result<Value, Error>`: Invokes script global function or registered host function.

Script Resolvers:
-----------------
- `DiskScriptResolver`: Reads files from local filesystem using `std::fs::read_to_string`.
- `MemoryScriptResolver::new(hashmap)`: Resolves imports from an in-memory `HashMap<String, String>`.

================================================================================
8. RECEIVER METHODS & CALLMETHOD MECHANICS
================================================================================

Script Syntax:
  type Zombie struct { HP int }
  func (z *Zombie) Hit(amount int) { z.HP -= amount }

Compiler & VM Mechanics:
1. Compilation: A receiver method declaration `func (z *Zombie) Hit(amount int)` is compiled into a global function named `"Zombie.Hit"`. The receiver parameter `z` becomes parameter 0.
2. Bytecode Opcode: Method calls (`z.Hit(25)`) emit `OpCode::CallMethod`.
3. Dispatch Sequence in VM for `CallMethod`:
   a. Pops arguments off stack.
   b. Pops receiver `Value::Struct(inst)` off stack.
   c. Looks up `type_name` (`"Zombie"`) and constructs function key `"Zombie.Hit"`.
   d. First checks host native functions (`host_fns["Zombie.Hit"]`). If found, invokes host closure with `receiver` as the first argument (`args[0]`).
   e. Otherwise, checks script globals (`globals["Zombie.Hit"]`). Pushes `receiver` then `args` onto VM stack and executes function frame.

Registering Rust-Native Receiver Methods:
------------------------------------------
```rust
vm.register_fn("Zombie.Hit", |args| {
    let receiver = &args[0]; // Value::Struct
    let amount = args[1].as_number().unwrap_or(0.0);
    // Mutate struct or execute host logic
    Value::Nil
});
```

================================================================================
9. HOT RELOADING ARCHITECTURE
================================================================================

`goscript::hot_reload::HotReloadEngine` enables live script editing without losing 
runtime global state.

```rust
use goscript::hot_reload::HotReloadEngine;

let mut engine = HotReloadEngine::new("scripts/player.go");

// On main thread frame tick:
if engine.reload_if_changed()? {
    println!("Script recompiled and live-swapped!");
}

engine.vm.call("OnUpdate", vec![Value::Float(dt)])?;
```

How Hot Reload Works:
1. Dependency Tracking: `resolve_imports_with_deps` recurses all `import` paths and returns a full set of dependency file paths (`dep_paths`).
2. File Modification Check: `reload_if_changed()` checks filesystem mtime of the main script and all recorded dependency paths.
3. Live Global State Preservation:
   - Before compiling the new code, `HotReloadEngine` extracts all active global variable names from `engine.vm.globals`.
   - `Compiler::with_preserved_globals(name, preserve_set)` suppresses emitting `SetGlobal` initialization for top-level `var` declarations that already exist in the VM.
   - Live script values (e.g. `player.hp = 42`) remain untouched while code/functions are replaced.

================================================================================
10. ENGINE INTEGRATION PATTERNS (BEVY / GROOT)
================================================================================

Threading & Single-Threaded Constraints:
- GoScript uses `Rc<RefCell<T>>` for single-threaded performance. These types are **`!Send` and `!Sync`**.
- Multi-threaded game engines (like Bevy) requiring `Send + Sync` on `Resource` objects must manage GoScript using **`NonSendMut<T>`** or thread-local storage / static Mutex wrappers.

Multi-Entity Shared Host Architecture (as demonstrated in `examples/groot`):
-----------------------------------------------------------------------------
1. Shared Bridge State (`ScriptBridgeState`):
   - Holds shared world data: entity states (pos, rot, color), input snapshot, event bus, debug gizmo queue, command queue.
   - Wrapped in `Rc<RefCell<ScriptBridgeState>>` or thread-safe equivalent.
2. Script Host (`GrootScriptHost`):
   - Keyed by script path (`HashMap<String, HotReloadEngine>`).
   - Shared VM instances across entities that share the same script file (avoids duplicating VM overhead).
3. Self-Context Dispatch (`CURRENT_ENTITY` Thread-Local):
   ```rust
   thread_local! {
       static CURRENT_ENTITY: Cell<u64> = Cell::new(0);
   }

   // Register self-context API:
   vm.register_fn("groot.GetSelfPosition", move |_| {
       let id = CURRENT_ENTITY.with(|c| c.get());
       let st = bridge.borrow();
       let state = st.entity_states.get(&EntityId(id)).cloned().unwrap_or_default();
       Value::Slice(Rc::new(RefCell::new(vec![Value::Float(state.x), Value::Float(state.y)])))
   });

   // Execution tick per entity:
   for (entity_id, script_path) in entities {
       CURRENT_ENTITY.with(|c| c.set(entity_id.0));
       engine.vm.set_delta_time(dt);
       engine.vm.call("OnUpdate", vec![Value::Float(dt)])?;
   }
   ```
4. Deferred Engine Commands:
   - Scripts calling `groot.SpawnEntity(...)` or `groot.DestroySelf()` push `EngineCommand` items to a deferred queue.
   - Command processing systems drain and execute commands AFTER all script `OnUpdate` calls complete for the frame, preventing borrow conflicts during execution.

================================================================================
11. COMMON GOTCHAS & ARCHITECTURAL RULES FOR LLMS / CONTRIBUTORS
================================================================================

Rule 1: Box Dereferencing in AST Pattern Matches
------------------------------------------------
Pay strict attention to whether an AST enum field is `Box<Expr>` or bare `Expr`:
- `Stmt::SetField { object, field, value }`: `object` is `Expr` (bare).
  Match: `if let Expr::Identifier(m) = &object`
- `Expr::GetField { object, field }`: `object` is `Box<Expr>`.
  Match: `if let Expr::Identifier(m) = &*object`
- `Expr::MethodCall { receiver, method, args }`: `receiver` is `Box<Expr>`.
  Match: `if let Expr::Identifier(m) = &*receiver`

Rule 2: Function Registration & Host Closures
---------------------------------------------
- Host functions use `Rc<dyn Fn(Vec<Value>) -> Value>`. Do NOT use bare function pointers `fn(Vec<Value>) -> Value`. Host closures frequently need to capture state (e.g. keyboard state, engine pointers).

Rule 3: Module Declarations & Library Exports
---------------------------------------------
- Adding `pub use foo::FooItem;` in `lib.rs` does NOT automatically expose the module. You MUST also write `pub mod foo;`.

Rule 4: Token Structure
-----------------------
- `Token` contains `kind: TokenKind`, `line: usize`, `col: usize`. There is NO `lexeme` field. String literals and identifiers store their strings directly inside `TokenKind::Identifier(String)` and `TokenKind::Str(String)`.

Rule 5: Function & Parser Free Functions
----------------------------------------
- Call `parser::parse(tokens)`, NOT `Parser::new(tokens).parse()`.
- Compiler instances are consumed: `Compiler::new(name).compile(&stmts)`.

Rule 6: Error Constructing
--------------------------
- Runtime errors with line info: `Error::runtime_at(msg, line)`.
- Generic runtime errors without line info: `Error::runtime(msg)`.
- Parse/Lex errors with line and column: `Error::new(msg, line, col)`.

Rule 7: Package Declarations are Process-Wide
--------------------------------------------
- `declare_package(name)` mutates a process-wide `OnceLock<Mutex<HashSet<String>>>`. Keep module and package names distinct in unit tests to avoid crosstalk across test runs.

Rule 8: Stmt Match Exhaustiveness
---------------------------------
- When adding a new `Stmt` variant, update ALL `match stmt` arms across the codebase:
  - `src/compiler.rs` (`compile_stmt`)
  - `src/resolver.rs` (`rewrite_import_refs`, `rewrite_self_refs`, `prefix_decl`)
```
