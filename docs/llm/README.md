# LLM Contributor Guide — GoScript

Technical notes, gotchas, and conventions for LLMs working on this codebase.

---

## 1. Architecture Rules (Never Violate)

- **Pasted "complete implementations" are feature specs, not production code.** Never overwrite existing modules with pasted monoliths. Adapt their feature sets into the real architecture.
- **`register_fn` holds closures, not function pointers.** Use `Rc<dyn Fn>` (not bare `fn` pointers) so host engines can register capturing closures (e.g. keyboard state per frame).
- **`pub use` in `lib.rs` does NOT auto-create public modules.** You must also add `pub mod foo;` before `pub use foo::FooItem;`.
- **This crate is a library + binary.** The binary (`main.rs`) is a demo; the library (`lib.rs`) is what hosts (groot/Bevy) embed.

---

## 2. File-Specific Gotchas

### `parser.rs`
- `parser::parse(tokens)` is a **free function**, not `Parser::new(tokens).parse()`. Do not use a method-based API.
- The `Token` struct has `kind`, `line`, `col` — there is no `lexeme` field.
- Dotted names are resolved at parse time via a global package registry: `declare_package("groot")` + `is_package("groot")`.
- `TokenKind::Identifier("range")` — the word `range` is NOT a keyword; it lexes as `Identifier`.
- `is_package` is called during `parse_postfix` on the **Identifier base** before consuming the dot.

### `value.rs`
- Use `.to_string()` for display. There is no `.display()` method.
- `Value` implements `fmt::Display` (which gives `.to_string()` for free).
- `Value::as_number() -> Option<f64>` — numeric coercion for ints and floats.
- `Value::as_string() -> Option<&str>` — string extraction for String variant.
- `Value` variants: `Nil`, `Int(i64)`, `Float(f64)`, `String(String)`, `Bool(bool)`, `Struct(Rc<RefCell<StructInstance>>)`, `Slice(Rc<RefCell<Vec<Value>>>)`, `Map(Rc<RefCell<HashMap<String, Value>>>)`, `Function(Rc<CompiledFunction>)`, `NativeFn(fn(Vec<Value>) -> Value)`.
- `StructInstance` is in `value.rs`, NOT in a separate `struct.rs`.

### `function.rs`
- `CompiledFunction` contains `code: Vec<u8>`, `constants: Vec<Value>`, `lines: Vec<usize>` (parallel to code for P1 line mapping).

### `compiler.rs`
- `Compiler::new(name)` takes `impl Into<String>` (a `&str` is fine).
- `Compiler::compile(self, stmts)` takes `self` by value (consumes the compiler).
- Methods: `emit_op`, `emit_byte`, `emit_constant`, `name_constant`, `resolve_local`.
- `name_constant` pushes a `Value::String` and returns the index.

### `vm.rs`
- `vm.compile(&self, source)` takes `&self` and returns `Result<Rc<CompiledFunction>, Error>`.
- `vm.execute(main_fn)` takes `Rc<CompiledFunction>`.
- `vm.call(name, args)` — the first arg is `name`, not "func".
- `vm.register_fn` and `vm.register_std_lib` take `&mut self`.
- Host engines typically call `vm.register_fn("groot.Log", ...)`.

### `hot_reload.rs`
- `HotReloadEngine::new(path)` takes `&str`.
- `engine.reload_if_changed()` returns `Result<bool, Error>`.
- Watches all imported dependencies recursively (tracked via `dep_paths` / `dep_modified`).
- Hot-reload preserves live global values across code swaps.

### `error.rs`
- `Error::runtime(msg)` — no line info.
- `Error::runtime_at(msg, line)` — anchored to a source line.
- `Error::new(msg, line, col)` — full error with line + col.

---

## 3. Box Deref Gotcha (Pattern Matching)

When pattern-matching on expressions, pay attention to whether the field is `Box<Expr>` or `Expr`:

```rust
// Stmt::SetField — object is Expr (NOT Box)
Stmt::SetField { object, field, value, line } => {
    if let Expr::Identifier(m) = &object { ... }  // ✅ correct
}

// Expr::GetField — object is Box<Expr>
Expr::GetField { object, field } => {
    if let Expr::Identifier(m) = &*object { ... }  // ✅ correct (dereference)
}

// Expr::MethodCall — receiver is Box<Expr>
Expr::MethodCall { receiver, method, args } => {
    if let Expr::Identifier(m) = &*receiver { ... }  // ✅ correct (dereference)
}
```

---

## 4. Bevy Integration (groot repo)

- `goscript` uses `Rc` and `RefCell` for single-threaded performance. These types are **not `Send`/`Sync`**.
- Bevy's `Resource`/`ResMut` requires `Send + Sync`. Use **`NonSendMut<T>`** and `insert_non_send_resource()` instead.
- `GrootScriptHost` does NOT derive `Resource`.
- Position crosses the VM↔Bevy boundary via a `static Mutex<(f32, f32)>` (native bindings can't borrow the Bevy world).

---

## 5. Module System Design Notes

### How it works
1. Parser emits `Stmt::Import { path, line }` for each import.
2. `resolver::resolve_imports(stmts, resolver)` resolves all imports recursively.
3. Imported files' top-level `FuncDecl`/`VarDecl` get prefixed with `module_name.`.
4. A **self-qualification pass** rewrites bare references to own top-level symbols inside the imported module.
5. A **dotted-ref pass** rewrites `submod.X` references to dotted globals.

### Key design decisions
- **Parse order**: Import paths aren't known until after parsing. Package declaration happens during `resolve_imports`, AFTER the parse. The dotted-ref rewrite handles the parse-vs-declaration ordering.
- **Receiver methods are NOT namespaced** (`func (z *Zombie) Hit()` stays `Zombie.Hit`). Only plain top-level funcs and vars get prefixed.
- **Struct types are compile-time metadata only** — `StructDecl` is a no-op in the compiler. Struct instances are dynamic field maps.
- **Circular import detection** uses a `HashSet<String>` of visited path strings (no normalization; exact string match).

### What to watch out for
- `declare_package` modifies a process-wide static (`OnceLock<Mutex<HashSet>>`). This persists across tests — keep module names unique in tests.
- Import paths in `MemoryScriptResolver` are matched exactly (no normalization). `"utils/a.gs"` and `"utils//a.gs"` are different keys.
- The `Stmt::Import` variant must be added to EVERY exhaustive match on `Stmt` — including `compiler.rs`, `resolver.rs`, and any new rewrite functions.

---

## 6. Adding a New Feature Checklist

1. Read the existing module you're modifying — mimic code style, naming, patterns.
2. Check if you're touching a `Box<Expr>` vs `Expr` field (see section 3).
3. Add the new variant to all exhaustive `Stmt`/`Expr` matches (search for `match stmt`, `match expr`).
4. Write tests using `MemoryScriptResolver` for import-related features.
5. Never assume a library is available — check `Cargo.toml` first.
6. Run `cargo test` and `cargo clippy` before committing.
7. Do not add `#![allow(...)]` to silence warnings — fix the root cause.

---

## 7. New Features (v0.2+)

### CallMethod host support
- `OpCode::CallMethod` checks `host_fns["TypeName.Method"]` before `globals`.
- Register Rust-native receiver methods as `"Zombie.Hit"`, `"Player.TakeDamage"`, etc.
- The receiver is passed as the first argument to the host function.

### Recursive hot reloading
- `HotReloadEngine` tracks all imported dependency paths via `resolve_imports_with_deps`.
- `reload_if_changed()` checks mtime of both the main script and all deps.
- Live global values are preserved across reloads.

### Standard library: strings package
- `strings.Contains(s, substr)`, `strings.ToLower(s)`, `strings.ToUpper(s)`
- `strings.HasPrefix(s, prefix)`, `strings.HasSuffix(s, suffix)`
- `strings.Trim(s, chars)`, `strings.Replace(s, old, new, n)`, `strings.Split(s, delim)`, `strings.Join(slice, sep)`

### Standard library: math extensions
- `math.Floor(x)`, `math.Ceil(x)`, `math.Round(x)`
- `math.Atan2(y, x)`, `math.Pow(base, exp)`

### Multi-entity architecture (examples/groot)
- `ScriptBridgeState` — shared world state (positions, input, command queue).
- `GrootScriptHost` — per-entity `HotReloadEngine` instances.
- `EngineCommand` — deferred commands: `SpawnEntity`, `DestroyEntity`, `SetPosition`, `PlaySound`.
- Systems: `system_sync_input`, `system_run_scripts`, `system_process_commands`.
