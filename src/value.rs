use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt;
use std::rc::Rc;

use crate::function::CompiledFunction;

#[derive(Debug, Clone, PartialEq)]
pub struct StructInstance {
    pub type_name: String,
    pub fields: HashMap<String, Value>,
}

impl StructInstance {
    pub fn new(type_name: impl Into<String>) -> Self {
        Self {
            type_name: type_name.into(),
            fields: HashMap::new(),
        }
    }
}

#[derive(Debug, Clone)]
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

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Value::Nil, Value::Nil) => true,
            (Value::Int(a), Value::Int(b)) => a == b,
            (Value::Float(a), Value::Float(b)) => a == b,
            (Value::String(a), Value::String(b)) => a == b,
            (Value::Bool(a), Value::Bool(b)) => a == b,
            (Value::Struct(a), Value::Struct(b)) => Rc::ptr_eq(a, b),
            (Value::Function(a), Value::Function(b)) => Rc::ptr_eq(a, b),
            _ => false,
        }
    }
}

impl Value {
    pub fn truthy(&self) -> bool {
        match self {
            Value::Nil => false,
            Value::Bool(b) => *b,
            Value::Int(i) => *i != 0,
            Value::Float(f) => *f != 0.0,
            Value::String(s) => !s.is_empty(),
            _ => true,
        }
    }

    pub fn as_number(&self) -> Option<f64> {
        match self {
            Value::Int(i) => Some(*i as f64),
            Value::Float(f) => Some(*f),
            _ => None,
        }
    }

    pub fn as_string(&self) -> Option<&str> {
        match self {
            Value::String(s) => Some(s),
            _ => None,
        }
    }
}

fn format_float(x: f64) -> String {
    if x == x.trunc() && x.abs() < 1e16 {
        format!("{}", x.trunc() as i64)
    } else {
        let mut s = x.to_string();
        if s == "-0" {
            s = "0".into();
        }
        s
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Nil => write!(f, "nil"),
            Value::Int(i) => write!(f, "{i}"),
            Value::Float(x) => write!(f, "{}", format_float(*x)),
            Value::String(s) => write!(f, "{s}"),
            Value::Bool(b) => write!(f, "{b}"),
            Value::Struct(instance) => {
                let inst = instance.borrow();
                if inst.fields.is_empty() {
                    write!(f, "{} {{}}", inst.type_name)
                } else {
                    let parts: Vec<String> = inst
                        .fields
                        .iter()
                        .map(|(k, v)| format!("{k}: {v}"))
                        .collect();
                    write!(f, "{} {{{}}}", inst.type_name, parts.join(", "))
                }
            }
            Value::Slice(slice) => {
                let items: Vec<String> = slice.borrow().iter().map(|v| v.to_string()).collect();
                write!(f, "[{}]", items.join(", "))
            }
            Value::Map(map) => {
                let items: Vec<String> = map
                    .borrow()
                    .iter()
                    .map(|(k, v)| format!("{k}: {v}"))
                    .collect();
                write!(f, "{{{}}}", items.join(", "))
            }
            Value::Function(func) => write!(f, "<func {}>", func.name),
            Value::NativeFn(_) => write!(f, "<native fn>"),
        }
    }
}