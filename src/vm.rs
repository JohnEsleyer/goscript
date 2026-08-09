use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use crate::compiler::Compiler;
use crate::error::Error;
use crate::function::CompiledFunction;
use crate::lexer::Lexer;
use crate::opcode::OpCode;
use crate::parser;
use crate::resolver::{resolve_imports, DiskScriptResolver, ScriptResolver};
use crate::value::{StructInstance, Value};

// ---------------------------------------------------------------------------
// Stateless engine clock/RNG shared by the native standard library. Pure
// functions can't capture VM state, so frame-delta and random state live in
// process-wide atomics (a game engine typically owns a single VM).
// ---------------------------------------------------------------------------

static START_TIME: OnceLock<Instant> = OnceLock::new();
static DELTA_TIME: AtomicU64 = AtomicU64::new(0); // f64::to_bits

/// Range-random float in [0.0, 1.0). A tiny xorshift64* for scripts; fast and
/// dependency-free, not cryptographically random.
fn rand_f64(state: &Cell<u64>) -> f64 {
    if state.get() == 0 {
        let seed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0x9E37_79B9_7F4A_7C15);
        state.set(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
    }
    let mut s = state.get();
    s ^= s >> 12;
    s ^= s << 25;
    s ^= s >> 27;
    state.set(s);
    (s >> 11) as f64 / (1u64 << 53) as f64
}

#[derive(Debug, Clone, Copy)]
enum ArithOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
}

struct CallFrame {
    function: Rc<CompiledFunction>,
    ip: usize,
    slots_offset: usize,
}

pub struct VirtualMachine {
    frames: Vec<CallFrame>,
    stack: Vec<Value>,
    pub globals: HashMap<String, Value>,
    /// Native host functions. `Rc<dyn Fn>` (unlike `fn` pointers) allows
    /// engines to register capturing closures, e.g. keyboard state captured
    /// per frame.
    host_fns: HashMap<String, Rc<dyn Fn(Vec<Value>) -> Value>>,
    /// P0 anti-freeze guard: cap on executed bytecode instructions per
    /// `execute`/`call`, so runaway scripts (e.g. `for {}`) surface a timeout
    /// error instead of hanging the host engine. `None` disables the limit.
    max_instructions: Option<u64>,
    instruction_count: u64,
    /// Per-VM xorshift64* random state (was process-wide static).
    /// Uses `Rc<Cell>` so closures can capture it without borrow conflicts
    /// with `host_fns` (interior mutability, no `&mut self` needed).
    rand_state: Rc<Cell<u64>>,
    /// Supplies imported script files (`import "path/file.gs"`). Defaults to
    /// the filesystem; host engines may override with embedded/virtual files.
    pub resolver: Rc<dyn ScriptResolver>,
}

impl VirtualMachine {
    pub fn new() -> Self {
        let mut vm = Self {
            frames: Vec::new(),
            stack: Vec::new(),
            globals: HashMap::new(),
            host_fns: HashMap::new(),
            max_instructions: Some(1_000_000),
            instruction_count: 0,
            rand_state: Rc::new(Cell::new(0)),
            resolver: Rc::new(DiskScriptResolver),
        };
        vm.register_std_lib();
        vm
    }

    /// Configures the anti-freeze instruction budget. `Some(n)` kills scripts
    /// after `n` executed bytecode instructions; `None` disables the guard.
    pub fn set_max_instructions(&mut self, max_instructions: Option<u64>) {
        self.max_instructions = max_instructions;
    }

    /// Replaces the file resolver used to load `import "path"` modules. By
    /// default scripts are read from disk; host engines can supply files from
    /// memory or virtual archives instead.
    pub fn set_resolver(&mut self, resolver: Rc<dyn ScriptResolver>) {
        self.resolver = resolver;
    }

    /// Injects the GoScript native standard library (`math`, `fmt`, `rand`,
    /// `time`) into the VM's global table under familiar Go package names.
    pub fn register_std_lib(&mut self) {
        // -------------------------------------------------------------------
        // built-ins (language-level, no package)
        // -------------------------------------------------------------------
        self.register_fn("len", |args| match args.first() {
            Some(Value::Slice(items)) => Value::Int(items.borrow().len() as i64),
            Some(Value::Map(map)) => Value::Int(map.borrow().len() as i64),
            Some(Value::String(s)) => Value::Int(s.len() as i64),
            _ => Value::Int(0),
        });

        self.register_fn("append", |args| match (args.get(0), args.get(1)) {
            (Some(Value::Slice(items)), Some(item)) => {
                items.borrow_mut().push(item.clone());
                Value::Slice(items.clone())
            }
            _ => Value::Nil,
        });

        // -------------------------------------------------------------------
        // explicit type casts: int(x), float64(x), string(x), bool(x)
        // -------------------------------------------------------------------
        self.register_fn("int", |args| match args.first() {
            Some(Value::Int(v)) => Value::Int(*v),
            Some(Value::Float(v)) => Value::Int(*v as i64),
            Some(Value::String(s)) => Value::Int(s.trim().parse().unwrap_or(0)),
            Some(Value::Bool(b)) => Value::Int(if *b { 1 } else { 0 }),
            _ => Value::Int(0),
        });

        self.register_fn("float64", |args| match args.first() {
            Some(Value::Int(v)) => Value::Float(*v as f64),
            Some(Value::Float(v)) => Value::Float(*v),
            Some(Value::String(s)) => Value::Float(s.trim().parse().unwrap_or(0.0)),
            _ => Value::Float(0.0),
        });

        self.register_fn("string", |args| match args.first() {
            Some(Value::String(s)) => Value::String(s.clone()),
            Some(v) => Value::String(v.to_string()),
            None => Value::String(String::new()),
        });

        self.register_fn("bool", |args| match args.first() {
            Some(v) => Value::Bool(v.truthy()),
            None => Value::Bool(false),
        });

        // -------------------------------------------------------------------
        // math
        // -------------------------------------------------------------------
        self.globals
            .insert("math.Pi".to_string(), Value::Float(std::f64::consts::PI));

        self.register_fn("math.Sqrt", |args| match args.first() {
            Some(Value::Float(f)) => Value::Float(f.sqrt()),
            Some(Value::Int(i)) => Value::Float((*i as f64).sqrt()),
            _ => Value::Nil,
        });

        self.register_fn("math.Abs", |args| match args.first() {
            Some(Value::Float(f)) => Value::Float(f.abs()),
            Some(Value::Int(i)) => Value::Int(i.checked_abs().unwrap_or(0)),
            _ => Value::Nil,
        });

        self.register_fn("math.Sin", |args| {
            num0(&args).map(|x| Value::Float(x.sin())).unwrap_or(Value::Nil)
        });

        self.register_fn("math.Cos", |args| {
            num0(&args).map(|x| Value::Float(x.cos())).unwrap_or(Value::Nil)
        });

        self.register_fn("math.Min", |args| match (args.get(0), args.get(1)) {
            (Some(Value::Int(a)), Some(Value::Int(b))) => Value::Int((*a).min(*b)),
            (Some(a), Some(b)) => match (a.as_number(), b.as_number()) {
                (Some(x), Some(y)) => Value::Float(x.min(y)),
                _ => Value::Nil,
            },
            _ => Value::Nil,
        });

        self.register_fn("math.Max", |args| match (args.get(0), args.get(1)) {
            (Some(Value::Int(a)), Some(Value::Int(b))) => Value::Int((*a).max(*b)),
            (Some(a), Some(b)) => match (a.as_number(), b.as_number()) {
                (Some(x), Some(y)) => Value::Float(x.max(y)),
                _ => Value::Nil,
            },
            _ => Value::Nil,
        });

        self.register_fn("math.Clamp", |args| {
            match (args.get(0), args.get(1), args.get(2)) {
                (Some(Value::Int(v)), Some(Value::Int(min)), Some(Value::Int(max))) => {
                    Value::Int((*v).clamp(*min, *max))
                }
                (Some(a), Some(b), Some(c)) => {
                    match (a.as_number(), b.as_number(), c.as_number()) {
                        (Some(v), Some(min), Some(max)) => Value::Float(v.clamp(min, max)),
                        _ => Value::Nil,
                    }
                }
                _ => Value::Nil,
            }
        });

        // -------------------------------------------------------------------
        // fmt
        // -------------------------------------------------------------------
        self.register_fn("fmt.Println", |args| {
            let output: Vec<String> = args.iter().map(|a| a.to_string()).collect();
            println!("[fmt.Println] {}", output.join(" "));
            Value::Nil
        });

        self.register_fn("fmt.Sprintf", |args| {
            let Some(Value::String(fmt_str)) = args.first() else {
                return Value::Nil;
            };
            let chars: Vec<char> = fmt_str.chars().collect();
            let mut out = String::new();
            let mut i = 0;
            let mut arg_idx = 1;
            while i < chars.len() {
                if chars[i] == '%' && i + 1 < chars.len() {
                    let spec = chars[i + 1];
                    if spec == '%' {
                        out.push('%');
                        i += 2;
                        continue;
                    }
                    match args.get(arg_idx) {
                        Some(arg) => {
                            out.push_str(&format_spec(spec, arg));
                            arg_idx += 1;
                        }
                        None => {
                            out.push('%');
                            out.push(spec);
                        }
                    }
                    i += 2;
                } else {
                    out.push(chars[i]);
                    i += 1;
                }
            }
            Value::String(out)
        });

        // -------------------------------------------------------------------
        // rand
        // -------------------------------------------------------------------
        let rand = Rc::clone(&self.rand_state);
        self.register_fn("rand.Float", move |_| Value::Float(rand_f64(&rand)));
        let rand = Rc::clone(&self.rand_state);
        self.register_fn("rand.Intn", move |args| match args.first() {
            Some(Value::Int(max)) if *max > 0 => {
                Value::Int((rand_f64(&rand) * *max as f64) as i64)
            }
            _ => Value::Int(0),
        });

        // -------------------------------------------------------------------
        // time
        // -------------------------------------------------------------------
        self.register_fn("time.Now", |_| {
            let start = *START_TIME.get_or_init(Instant::now);
            Value::Float(start.elapsed().as_secs_f64())
        });

        self.register_fn("time.Delta", |_| {
            Value::Float(f64::from_bits(DELTA_TIME.load(Ordering::Relaxed)))
        });
    }

    /// Records the current frame's delta time (seconds), returned by
    /// `time.Delta()` from scripts. Call this once per engine frame.
    pub fn set_delta_time(&mut self, dt: f64) {
        DELTA_TIME.store(dt.to_bits(), Ordering::Relaxed);
    }

    pub fn register_fn(
        &mut self,
        name: &str,
        f: impl Fn(Vec<Value>) -> Value + 'static,
    ) {
        if let Some(pkg) = name.split('.').next() {
            crate::parser::declare_package(pkg);
        }
        self.host_fns.insert(name.to_string(), Rc::new(f));
    }

    pub fn compile(&self, source: &str) -> Result<Rc<CompiledFunction>, Error> {
        let mut lexer = Lexer::new(source);
        let tokens = lexer.tokenize()?;
        let ast = parser::parse(tokens)?;
        let ast = resolve_imports(ast, self.resolver.as_ref())?;
        let compiled = Compiler::new("main").compile(&ast);
        Ok(Rc::new(compiled))
    }

    pub fn execute(&mut self, main_fn: Rc<CompiledFunction>) -> Result<(), Error> {
        self.instruction_count = 0;
        self.frames.clear();
        self.stack.clear();
        self.frames.push(CallFrame {
            function: main_fn,
            ip: 0,
            slots_offset: 0,
        });
        self.run()
    }

    pub fn call(&mut self, name: &str, args: Vec<Value>) -> Result<Value, Error> {
        if let Some(func) = self.host_fns.get(name) {
            return Ok(native_call(func, args));
        }
        match self.globals.get(name).cloned() {
            Some(Value::NativeFn(native)) => Ok(native(args)),
            Some(Value::Function(function)) => {
                let slots_offset = self.stack.len();
                for arg in args {
                    self.stack.push(arg);
                }
                self.frames.push(CallFrame {
                    function,
                    ip: 0,
                    slots_offset,
                });
                self.instruction_count = 0;
                self.run()?;
                Ok(self.stack.pop().unwrap_or(Value::Nil))
            }
            _ => Err(Error::runtime(format!(
                "call to undefined function '{name}'"
            ))),
        }
    }

    fn run(&mut self) -> Result<(), Error> {
        while let Some(mut frame) = self.frames.pop() {
            while frame.ip < frame.function.code.len() {
                if let Some(max) = self.max_instructions {
                    self.instruction_count += 1;
                    if self.instruction_count > max {
                        return Err(Error::runtime_at(
                            format!(
                                "instruction budget exceeded ({} instructions) - possible infinite loop",
                                max
                            ),
                            line_of(&frame),
                        ));
                    }
                }
                let op = OpCode::from(frame.function.code[frame.ip]);
                frame.ip += 1;
                match op {
                    OpCode::Constant => {
                        let idx = frame.function.code[frame.ip] as usize;
                        frame.ip += 1;
                        let value = frame.function.constants[idx].clone();
                        self.stack.push(value);
                    }
                    OpCode::Nil => self.stack.push(Value::Nil),
                    OpCode::True => self.stack.push(Value::Bool(true)),
                    OpCode::False => self.stack.push(Value::Bool(false)),
                    OpCode::Pop => {
                        self.stack.pop();
                    }
                    OpCode::Add => self.arith(ArithOp::Add)?,
                    OpCode::Sub => self.arith(ArithOp::Sub)?,
                    OpCode::Mul => self.arith(ArithOp::Mul)?,
                    OpCode::Div => self.arith(ArithOp::Div)?,
                    OpCode::Mod => self.arith(ArithOp::Rem)?,
                    OpCode::Equal => {
                        let b = self.stack.pop().unwrap_or(Value::Nil);
                        let a = self.stack.pop().unwrap_or(Value::Nil);
                        self.stack.push(Value::Bool(values_equal(&a, &b)));
                    }
                    OpCode::NotEqual => {
                        let b = self.stack.pop().unwrap_or(Value::Nil);
                        let a = self.stack.pop().unwrap_or(Value::Nil);
                        self.stack.push(Value::Bool(!values_equal(&a, &b)));
                    }
                    OpCode::Greater => self.compare(|x, y| x > y)?,
                    OpCode::GreaterEqual => self.compare(|x, y| x >= y)?,
                    OpCode::Less => self.compare(|x, y| x < y)?,
                    OpCode::LessEqual => self.compare(|x, y| x <= y)?,
                    OpCode::Negate => {
                        let v = self.stack.pop().unwrap_or(Value::Nil);
                        match v {
                            Value::Int(i) => self.stack.push(Value::Int(i.wrapping_neg())),
                            Value::Float(f) => self.stack.push(Value::Float(-f)),
                            other => {
                                return Err(Error::runtime_at(
                                    format!("cannot negate a non-numeric value ({other})"),
                                    line_of(&frame),
                                ));
                            }
                        }
                    }
                    OpCode::Not => {
                        let v = self.stack.pop().unwrap_or(Value::Nil);
                        self.stack.push(Value::Bool(!v.truthy()));
                    }
                    OpCode::GetGlobal => {
                        let idx = frame.function.code[frame.ip] as usize;
                        frame.ip += 1;
                        let name = string_constant(&frame.function, idx);
                        let value = self.globals.get(name).cloned().unwrap_or(Value::Nil);
                        self.stack.push(value);
                    }
                    OpCode::SetGlobal => {
                        let idx = frame.function.code[frame.ip] as usize;
                        frame.ip += 1;
                        let value = self.stack.pop().unwrap_or(Value::Nil);
                        let name = string_constant(&frame.function, idx).to_string();
                        self.globals.insert(name, value);
                    }
                    OpCode::GetLocal => {
                        let idx = frame.function.code[frame.ip] as usize;
                        frame.ip += 1;
                        let value = self
                            .stack
                            .get(frame.slots_offset + idx)
                            .cloned()
                            .unwrap_or(Value::Nil);
                        self.stack.push(value);
                    }
                    OpCode::SetLocal => {
                        let idx = frame.function.code[frame.ip] as usize;
                        frame.ip += 1;
                        let value = self.stack.pop().unwrap_or(Value::Nil);
                        let target = frame.slots_offset + idx;
                        if target < self.stack.len() {
                            self.stack[target] = value;
                        } else {
                            self.stack.push(value);
                        }
                    }
                    OpCode::JumpIfFalse => {
                        let delta = self.read_jump_delta(&mut frame);
                        let value = self.stack.pop().unwrap_or(Value::Nil);
                        if !value.truthy() {
                            frame.ip = jump_apply(frame.ip, delta)?;
                        }
                    }
                    OpCode::JumpIfFalseKeep => {
                        let delta = self.read_jump_delta(&mut frame);
                        let falsy = self
                            .stack
                            .last()
                            .map(|v| !v.truthy())
                            .unwrap_or(true);
                        if falsy {
                            frame.ip = jump_apply(frame.ip, delta)?;
                        }
                    }
                    OpCode::JumpIfTrueKeep => {
                        let delta = self.read_jump_delta(&mut frame);
                        let truthy = self.stack.last().map(|v| v.truthy()).unwrap_or(false);
                        if truthy {
                            frame.ip = jump_apply(frame.ip, delta)?;
                        }
                    }
                    OpCode::Jump => {
                        let delta = self.read_jump_delta(&mut frame);
                        frame.ip = jump_apply(frame.ip, delta)?;
                    }
                    OpCode::Call => {
                        let idx = frame.function.code[frame.ip] as usize;
                        frame.ip += 1;
                        let arg_count = frame.function.code[frame.ip] as usize;
                        frame.ip += 1;
                        let callee_name = string_constant(&frame.function, idx);
                        if let Some(func) = self.host_fns.get(callee_name) {
                            let mut args = Vec::with_capacity(arg_count);
                            for _ in 0..arg_count {
                                args.push(self.stack.pop().unwrap_or(Value::Nil));
                            }
                            args.reverse();
                            let result = native_call(func, args);
                            self.stack.push(result);
                            continue;
                        }
                        match self.globals.get(callee_name).cloned() {
                            Some(Value::NativeFn(native)) => {
                                let mut args = Vec::with_capacity(arg_count);
                                for _ in 0..arg_count {
                                    args.push(self.stack.pop().unwrap_or(Value::Nil));
                                }
                                args.reverse();
                                let result = native(args);
                                self.stack.push(result);
                            }
                            Some(Value::Function(function)) => {
                                let slots_offset = self.stack.len().saturating_sub(arg_count);
                                self.frames.push(frame);
                                frame = CallFrame {
                                    function,
                                    ip: 0,
                                    slots_offset,
                                };
                            }
                            _ => {
                                return Err(Error::runtime_at(
                                    format!("callee '{callee_name}' is not a function"),
                                    line_of(&frame),
                                ));
                            }
                        }
                    }
                    OpCode::Return => {
                        let result = self.stack.pop().unwrap_or(Value::Nil);
                        self.stack.truncate(frame.slots_offset);
                        self.stack.push(result);
                        break;
                    }
                    OpCode::NewStruct => {
                        let type_name_idx = frame.function.code[frame.ip] as usize;
                        frame.ip += 1;
                        let field_count = frame.function.code[frame.ip] as usize;
                        frame.ip += 1;
                        let type_name =
                            string_constant(&frame.function, type_name_idx).to_string();
                        let mut instance = StructInstance::new(type_name);
                        for _ in 0..field_count {
                            let value = self.stack.pop().unwrap_or(Value::Nil);
                            let name = match self.stack.pop() {
                                Some(Value::String(s)) => s,
                                other => {
                                    return Err(Error::runtime(format!(
                                        "struct field name expected a string, got {}",
                                        other.map(|v| v.to_string()).unwrap_or_default()
                                    )));
                                }
                            };
                            instance.fields.insert(name, value);
                        }
                        self.stack
                            .push(Value::Struct(Rc::new(RefCell::new(instance))));
                    }
                    OpCode::NewSlice => {
                        let count = frame.function.code[frame.ip] as usize;
                        frame.ip += 1;
                        let mut items = Vec::with_capacity(count);
                        for _ in 0..count {
                            items.push(self.stack.pop().unwrap_or(Value::Nil));
                        }
                        items.reverse();
                        self.stack.push(Value::Slice(Rc::new(RefCell::new(items))));
                    }
                    OpCode::NewMap => {
                        let count = frame.function.code[frame.ip] as usize;
                        frame.ip += 1;
                        let mut map = HashMap::new();
                        for _ in 0..count {
                            let value = self.stack.pop().unwrap_or(Value::Nil);
                            let key = self.stack.pop().unwrap_or(Value::Nil);
                            let key = match key {
                                Value::String(s) => s,
                                other => other.to_string(),
                            };
                            map.insert(key, value);
                        }
                        self.stack.push(Value::Map(Rc::new(RefCell::new(map))));
                    }
                    OpCode::GetIndex => {
                        let index = self.stack.pop().unwrap_or(Value::Nil);
                        let target = self.stack.pop().unwrap_or(Value::Nil);
                        let value = match (target, index) {
                            (Value::Slice(items), Value::Int(i)) => items
                                .borrow()
                                .get(i as usize)
                                .cloned()
                                .unwrap_or(Value::Nil),
                            (Value::Map(map), Value::String(k)) => map
                                .borrow()
                                .get(&k)
                                .cloned()
                                .unwrap_or(Value::Nil),
                            (Value::Map(map), other) => map
                                .borrow()
                                .get(&other.to_string())
                                .cloned()
                                .unwrap_or(Value::Nil),
                            _ => Value::Nil,
                        };
                        self.stack.push(value);
                    }
                    OpCode::SetIndex => {
                        let value = self.stack.pop().unwrap_or(Value::Nil);
                        let index = self.stack.pop().unwrap_or(Value::Nil);
                        let target = self.stack.pop().unwrap_or(Value::Nil);
                        match (target, index) {
                            (Value::Slice(items), Value::Int(i)) => {
                                let mut items = items.borrow_mut();
                                let idx = i as usize;
                                if idx < items.len() {
                                    items[idx] = value;
                                } else {
                                    return Err(Error::runtime_at(
                                        format!(
                                            "index {idx} out of bounds for slice of length {}",
                                            items.len()
                                        ),
                                        line_of(&frame),
                                    ));
                                }
                            }
                            (Value::Map(map), Value::String(k)) => {
                                map.borrow_mut().insert(k, value);
                            }
                            (Value::Map(map), other) => {
                                map.borrow_mut().insert(other.to_string(), value);
                            }
                            _ => {
                                return Err(Error::runtime_at(
                                    "cannot index into a non-collection value",
                                    line_of(&frame),
                                ));
                            }
                        }
                    }
                    OpCode::CallMethod => {
                        let method_idx = frame.function.code[frame.ip] as usize;
                        frame.ip += 1;
                        let arg_count = frame.function.code[frame.ip] as usize;
                        frame.ip += 1;
                        let method = string_constant(&frame.function, method_idx).to_string();
                        let mut args = Vec::with_capacity(arg_count);
                        for _ in 0..arg_count {
                            args.push(self.stack.pop().unwrap_or(Value::Nil));
                        }
                        args.reverse();
                        let receiver = self.stack.pop().unwrap_or(Value::Nil);
                        let type_name = match &receiver {
                            Value::Struct(inst) => inst.borrow().type_name.clone(),
                            _ => {
                                return Err(Error::runtime_at(
                                    format!(
                                        "cannot call method '{method}' on a non-struct value"
                                    ),
                                    line_of(&frame),
                                ));
                            }
                        };
                        // Methods are registered as `<TypeName>.<Method>` globals
                        // with the receiver bound as their first parameter.
                        let fn_name = format!("{type_name}.{method}");
                        match self.globals.get(&fn_name).cloned() {
                            Some(Value::Function(function)) => {
                                let slots_offset = self.stack.len();
                                self.stack.push(receiver);
                                for arg in args {
                                    self.stack.push(arg);
                                }
                                self.frames.push(frame);
                                frame = CallFrame {
                                    function,
                                    ip: 0,
                                    slots_offset,
                                };
                            }
                            _ => {
                                return Err(Error::runtime_at(
                                    format!("method '{method}' is not defined for '{type_name}'"),
                                    line_of(&frame),
                                ));
                            }
                        }
                    }
                    OpCode::GetField => {
                        let idx = frame.function.code[frame.ip] as usize;
                        frame.ip += 1;
                        let field = string_constant(&frame.function, idx).to_string();
                        match self.stack.pop() {
                            Some(Value::Struct(instance)) => {
                                let value = instance
                                    .borrow()
                                    .fields
                                    .get(&field)
                                    .cloned()
                                    .unwrap_or(Value::Nil);
                                self.stack.push(value);
                            }
                            other => {
                                return Err(Error::runtime_at(
                                    format!(
                                        "cannot read field '{field}' from {}",
                                        other
                                            .map(|v| v.to_string())
                                            .unwrap_or_else(|| "nil".into())
                                    ),
                                    line_of(&frame),
                                ));
                            }
                        }
                    }
                    OpCode::SetField => {
                        let idx = frame.function.code[frame.ip] as usize;
                        frame.ip += 1;
                        let field = string_constant(&frame.function, idx).to_string();
                        let value = self.stack.pop().unwrap_or(Value::Nil);
                        match self.stack.pop() {
                            Some(Value::Struct(instance)) => {
                                instance.borrow_mut().fields.insert(field, value);
                            }
                            other => {
                                return Err(Error::runtime_at(
                                    format!(
                                        "cannot set field '{field}' on {}",
                                        other
                                            .map(|v| v.to_string())
                                            .unwrap_or_else(|| "nil".into())
                                    ),
                                    line_of(&frame),
                                ));
                            }
                        }
                    }
                }
            }
        }
        Ok(())
    }

    fn arith(&mut self, op: ArithOp) -> Result<(), Error> {
        let b = self.stack.pop().unwrap_or(Value::Nil);
        let a = self.stack.pop().unwrap_or(Value::Nil);
        let result = match (&a, &b) {
            (Value::Int(x), Value::Int(y)) => {
                let value = match op {
                    ArithOp::Add => x.wrapping_add(*y),
                    ArithOp::Sub => x.wrapping_sub(*y),
                    ArithOp::Mul => x.wrapping_mul(*y),
                    ArithOp::Div => {
                        if *y == 0 {
                            return Err(Error::runtime("integer division by zero"));
                        }
                        x.wrapping_div(*y)
                    }
                    ArithOp::Rem => {
                        if *y == 0 {
                            return Err(Error::runtime("integer remainder by zero"));
                        }
                        x.wrapping_rem(*y)
                    }
                };
                Value::Int(value)
            }
            (Value::Float(x), Value::Float(y)) => Value::Float(num_op(op, *x, *y)),
            (Value::Float(x), Value::Int(y)) => Value::Float(num_op(op, *x, *y as f64)),
            (Value::Int(x), Value::Float(y)) => Value::Float(num_op(op, *x as f64, *y)),
            (Value::String(x), Value::String(y)) if matches!(op, ArithOp::Add) => {
                Value::String(format!("{x}{y}"))
            }
            (Value::String(x), other) if matches!(op, ArithOp::Add) => {
                Value::String(format!("{x}{other}"))
            }
            (other, Value::String(y)) if matches!(op, ArithOp::Add) => {
                Value::String(format!("{other}{y}"))
            }
            (a, b) => {
                return Err(Error::runtime(format!(
                    "cannot apply arithmetic to {} and {}",
                    a, b
                )));
            }
        };
        self.stack.push(result);
        Ok(())
    }

    fn compare(&mut self, f: impl Fn(f64, f64) -> bool) -> Result<(), Error> {
        let b = self.stack.pop().unwrap_or(Value::Nil);
        let a = self.stack.pop().unwrap_or(Value::Nil);
        let result = match (a.as_number(), b.as_number()) {
            (Some(x), Some(y)) => Value::Bool(f(x, y)),
            _ => Value::Bool(false),
        };
        self.stack.push(result);
        Ok(())
    }

    fn read_jump_delta(&self, frame: &mut CallFrame) -> isize {
        let byte = frame.function.code[frame.ip];
        frame.ip += 1;
        byte as i8 as isize
    }
}

/// Invokes a stored host closure. Keeps the call sites tidy and avoids
/// needing `Rc<dyn Fn>` to be directly callable.
fn native_call(func: &Rc<dyn Fn(Vec<Value>) -> Value>, args: Vec<Value>) -> Value {
    func.as_ref()(args)
}

fn num_op(op: ArithOp, x: f64, y: f64) -> f64 {
    match op {
        ArithOp::Add => x + y,
        ArithOp::Sub => x - y,
        ArithOp::Mul => x * y,
        ArithOp::Div => x / y,
        ArithOp::Rem => x % y,
    }
}

/// Numeric coercion for single-argument stdlib math functions.
fn num0(args: &[Value]) -> Option<f64> {
    args.first().and_then(|v| v.as_number())
}

/// Formats one argument per a printf-style specifier. Supports the common
/// subset: `%v` (default), `%d`, `%f` (6 decimals), `%s`.
fn format_spec(spec: char, arg: &Value) -> String {
    match spec {
        'd' | 'i' => match arg {
            Value::Int(v) => v.to_string(),
            Value::Float(f) => format!("{}", *f as i64),
            other => other.to_string(),
        },
        'f' => match arg {
            Value::Float(v) => format!("{v:.6}"),
            Value::Int(v) => format!("{v}.000000"),
            other => other.to_string(),
        },
        's' => match arg {
            Value::String(s) => s.clone(),
            other => other.to_string(),
        },
        _ => arg.to_string(),
    }
}

fn jump_apply(ip: usize, delta: isize) -> Result<usize, Error> {
    let next = ip as isize + delta;
    if next < 0 {
        return Err(Error::runtime("jump out of bounds"));
    }
    Ok(next as usize)
}

fn values_equal(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Nil, Value::Nil) => true,
        (Value::Bool(x), Value::Bool(y)) => x == y,
        (Value::String(x), Value::String(y)) => x == y,
        (Value::Int(x), Value::Int(y)) => x == y,
        (Value::Float(x), Value::Float(y)) => x == y,
        // Cross-numeric: coerce to f64 for comparison.
        (Value::Int(_), Value::Float(_)) | (Value::Float(_), Value::Int(_)) => {
            a.as_number() == b.as_number()
        }
        // Struct/Slice/Map: defer to Value::eq which uses Rc::ptr_eq (reference
        // equality for aliased structs) and structural equality for collections.
        _ => a == b,
    }
}

fn string_constant<'a>(function: &'a CompiledFunction, idx: usize) -> &'a str {
    match function.constants.get(idx) {
        Some(Value::String(s)) => s.as_str(),
        _ => "",
    }
}

/// Source line of the instruction the given frame is currently executing
/// (P1 line mapping). Falls back to 0 when no line info is available.
fn line_of(frame: &CallFrame) -> usize {
    frame
        .function
        .lines
        .get(frame.ip.saturating_sub(1))
        .copied()
        .unwrap_or(0)
}

impl Default for VirtualMachine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hot_reload::HotReloadEngine;
    use crate::resolver::MemoryScriptResolver;
    use std::fs;
    use std::path::PathBuf;

    fn script_returns(src: &str, func: &str, args: Vec<Value>) -> String {
        let mut vm = VirtualMachine::new();
        let chunk = vm.compile(src).unwrap();
        vm.execute(chunk).unwrap();
        vm.call(func, args).unwrap().to_string()
    }

    #[test]
    fn structs_share_references() {
        let src = r#"
        type Point struct { hp int }
        var p = Point{hp: 100}
        var alias = p
        func Hit(n int) int {
            p.hp = p.hp - n
            return alias.hp
        }
        "#;
        assert_eq!(script_returns(src, "Hit", vec![Value::Int(40)]), "60");
    }

    #[test]
    fn for_loop_with_break_and_continue() {
        let src = r#"
var total = 0
for i := 0; i < 10; i = i + 1 {
    if i == 3 {
        continue
    }
    if i == 8 {
        break
    }
    total = total + i
}
func GetTotal() int { return total }
"#;
        // Skips 3, stops before 8: 0+1+2+4+5+6+7
        assert_eq!(script_returns(src, "GetTotal", vec![]), "25");
    }

    #[test]
    fn while_style_loop() {
        let src = r#"
var n = 5
var out = ""
for n > 0 {
    out = out + "x"
    n = n - 1
}
func Result() string { return out }
"#;
        assert_eq!(script_returns(src, "Result", vec![]), "xxxxx");
    }

    #[test]
    fn if_else_chain() {
        let src = r#"
func Classify(n int) string {
    if n > 10 { return "big" }
    if n > 5 { return "medium" }
    return "small"
}
"#;
        let mut vm = VirtualMachine::new();
        let chunk = vm.compile(src).unwrap();
        vm.execute(chunk).unwrap();
        assert_eq!(vm.call("Classify", vec![Value::Int(12)]).unwrap().to_string(), "big");
        assert_eq!(vm.call("Classify", vec![Value::Int(7)]).unwrap().to_string(), "medium");
        assert_eq!(vm.call("Classify", vec![Value::Int(2)]).unwrap().to_string(), "small");
    }

    #[test]
fn arithmetic_and_precedence() {
        let src = r#"func Add(a int) int { return 2 + 3 * 4 - 5 }"#;
        assert_eq!(script_returns(src, "Add", vec![]), "9");
    }

    #[test]
    fn division_by_zero_is_an_error() {
        let src = "var x = 10 / 0\n";
        let mut vm = VirtualMachine::new();
        let chunk = vm.compile(src).unwrap();
        let err = vm.execute(chunk).unwrap_err();
        assert!(err.message.contains("division by zero"));
    }

    #[test]
    fn string_concat_and_logic() {
        let src = r#"
func Combine(a string, b string) string {
    if a != "" && b != "" {
        return a + b
    }
    return "empty"
}
"#;
        assert_eq!(
            script_returns(src, "Combine", vec![Value::String("foo".into()), Value::String("bar".into())]),
            "foobar"
        );
        assert_eq!(
            script_returns(src, "Combine", vec![Value::String("foo".into()), Value::String("".into())]),
            "empty"
        );
    }

    #[test]
    fn host_fns_support_capturing_closures_and_packages() {
        let mut vm = VirtualMachine::new();
        let offset = 2.0;
        vm.register_fn("groot.Factor", move |args| {
            let base = args.first().and_then(|v| v.as_number()).unwrap_or(0.0);
            Value::Float(base * offset)
        });

        // Because `groot` was auto-declared, dotted member access compiles to
        // a dotted call instead of a struct field read.
        let src = r#"func Scale(v float64) float64 { return groot.Factor(v) }"#;
        let chunk = vm.compile(src).unwrap();
        vm.execute(chunk).unwrap();
        assert_eq!(
            vm.call("Scale", vec![Value::Float(3.0)]).unwrap().to_string(),
            "6"
        );
    }

    #[test]
    fn v5_compound_and_increment() {
        let src = r#"
func Sum() int {
    var total = 10
    total += 5
    total -= 3
    total *= 2
    total /= 3
    for i := 0; i < 5; i++ {
        total += 1
        total++
    }
    return total
}
"#;
        // 10 + 5 - 3 = 12; *2 = 24; /3 = 8; then (5 iterations * 2) = 18
        assert_eq!(script_returns(src, "Sum", vec![]), "18");
    }

    #[test]
    fn v5_slices_len_append_index() {
        let src = r#"
func Gather() int {
    var nums = []int{1, 2, 3}
    nums = append(nums, 4)
    nums[0] = 10
    return len(nums)*100 + nums[0] + nums[3]
}
"#;
        // 4*100 + 10 + 4 = 414
        assert_eq!(script_returns(src, "Gather", vec![]), "414");
    }

    #[test]
    fn v5_map_literals_and_lookup() {
        let src = r#"
func Wallet() int {
    var inv = map[string]int{"potions": 3, "gold": 100}
    inv["gold"] += 20
    inv["potions"] = 5
    return inv["potions"] + inv["gold"]
}
func Size() int { return len(map[string]int{"a": 1, "b": 2}) }
"#;
        assert_eq!(script_returns(src, "Wallet", vec![]), "125");
        assert_eq!(script_returns(src, "Size", vec![]), "2");
    }

    #[test]
    fn v5_receiver_methods() {
        let src = r#"
type Zombie struct { HP int }
func (z *Zombie) Hit(amount int) { z.HP -= amount }
func (z *Zombie) Alive() int { if z.HP > 0 { return 1 } return 0 }

func Combat() int {
    var z = Zombie{HP: 40}
    z.Hit(25)
    z.HP += 5
    return z.HP*10 + z.Alive()
}
"#;
        // 20 -> 200 + 1
        assert_eq!(script_returns(src, "Combat", vec![]), "201");
    }

    #[test]
    fn v5_switch_statement() {
        let src = r#"
func Classify(state string) string {
    var label = "?"
    switch state {
    case "IDLE":
        label = "waiting"
    case "ATTACK":
        label = "burning"
    default:
        label = "lost"
    }
    return label
}
"#;
        assert_eq!(script_returns(src, "Classify", vec![Value::String("ATTACK".into())]), "burning");
        assert_eq!(script_returns(src, "Classify", vec![Value::String("IDLE".into())]), "waiting");
        assert_eq!(script_returns(src, "Classify", vec![Value::String("REST".into())]), "lost");
    }

    #[test]
    fn bool_negation_stays_working() {
        let src = r#"
func IsAlive(hp int) int {
    if !(hp <= 0) { return 1 }
    return 0
}
"#;
        assert_eq!(script_returns(src, "IsAlive", vec![Value::Int(30)]), "1");
        assert_eq!(script_returns(src, "IsAlive", vec![Value::Int(0)]), "0");
    }

    #[test]
    fn anti_freeze_guard_aborts_infinite_loop() {
        let src = "func Spin() int { var n = 0\n for { n = n + 1 }\n return n }\n";
        let mut vm = VirtualMachine::new();
        vm.set_max_instructions(Some(100));
        let chunk = vm.compile(src).unwrap();
        vm.execute(chunk).unwrap();
        let err = vm.call("Spin", vec![]).unwrap_err();
        assert!(err.message.contains("instruction budget exceeded"));
        assert!(err.line > 0);
        // Restore unlimited so sibling tests keep their budget.
        vm.set_max_instructions(None);
    }

    #[test]
    fn runtime_errors_carry_source_lines() {
        let src = "var nums = []int{1, 2, 3}\nvar boom = 0\nfunc Steal() int {\n    nums[9] = 1\n    return 0\n}\n";
        let mut vm = VirtualMachine::new();
        let chunk = vm.compile(src).unwrap();
        vm.execute(chunk).unwrap();
        let err = vm.call("Steal", vec![]).unwrap_err();
        assert!(err.message.contains("out of bounds"));
        assert_eq!(err.line, 4, "expected the failing statement's line");
    }

    #[test]
    fn range_loop_over_slice() {
        let src = r#"
func Sum() int {
    var total = 0
    var nums = []int{3, 5, 7}
    for i, v := range nums {
        total = total + v
    }
    return total
}
func Count() int {
    var n = 0
    var names = []string{"a", "b", "c", "d"}
    for _i := range names {
        n = n + 1
    }
    return n
}
"#;
        assert_eq!(script_returns(src, "Sum", vec![]), "15");
        assert_eq!(script_returns(src, "Count", vec![]), "4");
    }

    #[test]
    fn explicit_type_casts() {
        let src = r#"
func ToInts() int { return int(3.9) }
func ToFloats() float64 { return float64(2) }
func ToStrings() string { return string(42) }
func ToBools() int {
    if bool(1) && !bool(0) { return 1 }
    return 0
}
func FromText() int { return int("7") }
"#;
        assert_eq!(script_returns(src, "ToInts", vec![]), "3");
        assert_eq!(script_returns(src, "ToFloats", vec![]), "2");
        assert_eq!(script_returns(src, "ToStrings", vec![]), "42");
        assert_eq!(script_returns(src, "ToBools", vec![]), "1");
        assert_eq!(script_returns(src, "FromText", vec![]), "7");
    }

    #[test]
    fn imports_resolve_modules_and_namespace() {
        let mut files = HashMap::new();
        files.insert(
            "utils/math_helpers.gs".to_string(),
            r#"
            func Lerp(a float64, b float64, t float64) float64 {
                return a + (b - a) * t
            }
            func Scale() int { return 2 }
            "#.to_string(),
        );
        let mut vm = VirtualMachine::new();
        vm.set_resolver(Rc::new(MemoryScriptResolver::new(files)));
        let src = r#"
        import "utils/math_helpers.gs"
        func Use() float64 { return math_helpers.Lerp(10.0, 100.0, 0.5) }
        func UseVar() int { return math_helpers.Scale() }
        "#;
        let chunk = vm.compile(src).unwrap();
        vm.execute(chunk).unwrap();
        assert_eq!(vm.call("Use", vec![]).unwrap().to_string(), "55");
        assert_eq!(vm.call("UseVar", vec![]).unwrap().to_string(), "2");
    }

    #[test]
    fn grouped_imports() {
        let mut files = HashMap::new();
        files.insert("a.gs".to_string(), "func Alpha() int { return 1 }\n".to_string());
        files.insert("b.gs".to_string(), "func Beta() int { return 2 }\n".to_string());
        let mut vm = VirtualMachine::new();
        vm.set_resolver(Rc::new(MemoryScriptResolver::new(files)));
        let src = r#"
        import (
            "a.gs"
            "b.gs"
        )
        func Sum() int { return a.Alpha() + b.Beta() }
        "#;
        let chunk = vm.compile(src).unwrap();
        vm.execute(chunk).unwrap();
        assert_eq!(vm.call("Sum", vec![]).unwrap().to_string(), "3");
    }

    #[test]
    fn imported_module_globals_and_self_references() {
        let mut files = HashMap::new();
        files.insert(
            "counter.gs".to_string(),
            "var count = 10\nfunc Bump(n int) int {\n count = count + n\n return count }\n"
                .to_string(),
        );
        let mut vm = VirtualMachine::new();
        vm.set_resolver(Rc::new(MemoryScriptResolver::new(files)));
        let src = r#"
        import "counter.gs"
        func Read() int { return counter.count }
        func Add() int { return counter.Bump(5) }
        "#;
        let chunk = vm.compile(src).unwrap();
        vm.execute(chunk).unwrap();
        assert_eq!(vm.call("Add", vec![]).unwrap().to_string(), "15");
        assert_eq!(vm.call("Read", vec![]).unwrap().to_string(), "15");
    }

    #[test]
    fn circular_imports_are_rejected() {
        let mut files = HashMap::new();
        files.insert(
            "a.gs".to_string(),
            "import \"b.gs\"\nfunc A() int { return 1 }\n".to_string(),
        );
        files.insert(
            "b.gs".to_string(),
            "import \"a.gs\"\nfunc B() int { return 2 }\n".to_string(),
        );
        let mut vm = VirtualMachine::new();
        vm.set_resolver(Rc::new(MemoryScriptResolver::new(files)));
        let src = "import \"a.gs\"\nfunc Main() int { return 0 }\n";
        let err = vm.compile(src).unwrap_err();
        assert!(err.message.contains("circular import"), "{}", err.message);
    }

    #[test]
    fn unresolved_import_is_an_error() {
        let mut vm = VirtualMachine::new();
        vm.set_resolver(Rc::new(MemoryScriptResolver::new(HashMap::new())));
        let src = "import \"missing.gs\"\nfunc Main() int { return 0 }\n";
        let err = vm.compile(src).unwrap_err();
        assert!(err.message.contains("missing.gs"), "{}", err.message);
    }

    #[test]
    fn hot_reload_preserves_global_state() {
        let path = PathBuf::from(std::env::temp_dir()).join("goscript_test_reload.gs");
        fs::write(&path, "var value = 7\nfunc GetValue() int { return value }\n").unwrap();
        let mut engine = HotReloadEngine::new(path.to_str().unwrap());
        engine.reload_if_changed().unwrap();

        fs::write(&path, "var value = 999\nfunc GetValue() int { return value * 2 }\n").unwrap();
        engine.reload_if_changed().unwrap();

        let value = engine.vm.call("GetValue", vec![]).unwrap();
        assert_eq!(value.to_string(), "14");

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn stdlib_math() {
        let src = r#"
func Root() float64 { return math.Sqrt(16.0) }
func MinOf() int { return math.Min(3, 8) }
func MaxOf() int { return math.Max(3, 8) }
func ClampIt() float64 { return math.Clamp(7.5, 0.0, 5.0) }
func ClampInt() int { return math.Clamp(42, 0, 10) }
func AbsNeg() int { return math.Abs(-5) }
func Pi() float64 { return math.Pi }
"#;
        assert_eq!(script_returns(src, "Root", vec![]), "4");
        assert_eq!(script_returns(src, "MinOf", vec![]), "3");
        assert_eq!(script_returns(src, "MaxOf", vec![]), "8");
        assert_eq!(script_returns(src, "ClampIt", vec![]), "5");
        assert_eq!(script_returns(src, "ClampInt", vec![]), "10");
        assert_eq!(script_returns(src, "AbsNeg", vec![]), "5");
        let pi: f64 = script_returns(src, "Pi", vec![]).parse().unwrap();
        assert!((pi - std::f64::consts::PI).abs() < 1e-12);
    }

    #[test]
    fn stdlib_fmt() {
        let src = r#"
func Greet() string { return fmt.Sprintf("hp=%d f=%f s=%s", 100, 2.5, "hero") }
func Percent() string { return fmt.Sprintf("100%%") }
"#;
        assert_eq!(
            script_returns(src, "Greet", vec![]),
            "hp=100 f=2.500000 s=hero"
        );
        assert_eq!(script_returns(src, "Percent", vec![]), "100%");
    }

    #[test]
    fn stdlib_rand_is_bounded() {
        let src = r#"
func Roll(max int) int { return rand.Intn(max) }
func Dice() float64 { return rand.Float() }
"#;
        for _ in 0..50 {
            let v: i64 = script_returns(src, "Roll", vec![Value::Int(10)]).parse().unwrap();
            assert!((0..10).contains(&v));
            let f: f64 = script_returns(src, "Dice", vec![]).parse().unwrap();
            assert!((0.0..1.0).contains(&f));
        }
    }

    #[test]
    fn stdlib_time_delta() {
        let src = r#"func Tick() float64 { return time.Delta() }"#;
        let mut vm = VirtualMachine::new();
        vm.set_delta_time(0.5);
        let chunk = vm.compile(src).unwrap();
        vm.execute(chunk).unwrap();
        let delta = vm.call("Tick", vec![]).unwrap().to_string();
        assert_eq!(delta, "0.5");
        // time.Now is monotonic and non-negative.
        let src2 = r#"func Now() float64 { return time.Now() }"#;
        let mut vm2 = VirtualMachine::new();
        let c2 = vm2.compile(src2).unwrap();
        vm2.execute(c2).unwrap();
        let now: f64 = vm2.call("Now", vec![]).unwrap().to_string().parse().unwrap();
        assert!(now >= 0.0);
    }
}