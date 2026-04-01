//! FuseValue — the universal runtime value type for compiled Fuse programs.
//!
//! Every Fuse value at runtime is a FuseValue. The compiler generates code
//! that creates, manipulates, and destructs FuseValues via runtime calls.

use std::collections::HashMap;
use std::fmt;

/// The universal tagged union for all Fuse runtime values.
#[derive(Clone)]
pub enum FuseValue {
    Int(i64),
    Float(f64),
    Bool(bool),
    Str(String),
    Unit,
    List(Vec<FuseValue>),
    Struct(FuseStruct),
    Enum(FuseEnum),
    Fn(FuseFn),
    Lambda(FuseLambda),
}

/// A Fuse struct instance.
#[derive(Clone)]
pub struct FuseStruct {
    pub type_name: String,
    pub fields: Vec<(String, FuseValue)>,
    pub del_fn: Option<String>,  // name of __del__ function, if any
}

/// A Fuse enum variant.
#[derive(Clone)]
pub struct FuseEnum {
    pub enum_name: String,
    pub variant: String,
    pub value: Option<Box<FuseValue>>,
}

/// A reference to a compiled Fuse function (by name).
#[derive(Clone)]
pub struct FuseFn {
    pub name: String,
}

/// A lambda closure (function pointer index + captured environment).
#[derive(Clone)]
pub struct FuseLambda {
    pub id: usize,
    pub captures: Vec<(String, FuseValue)>,
}

// ═══════════════════════════════════════════════════════════════════════
// Convenience constructors
// ═══════════════════════════════════════════════════════════════════════

impl FuseValue {
    pub fn none() -> Self {
        FuseValue::Enum(FuseEnum {
            enum_name: "Option".into(), variant: "None".into(), value: None,
        })
    }

    pub fn some(v: FuseValue) -> Self {
        FuseValue::Enum(FuseEnum {
            enum_name: "Option".into(), variant: "Some".into(), value: Some(Box::new(v)),
        })
    }

    pub fn ok(v: FuseValue) -> Self {
        FuseValue::Enum(FuseEnum {
            enum_name: "Result".into(), variant: "Ok".into(), value: Some(Box::new(v)),
        })
    }

    pub fn err(v: FuseValue) -> Self {
        FuseValue::Enum(FuseEnum {
            enum_name: "Result".into(), variant: "Err".into(), value: Some(Box::new(v)),
        })
    }

    pub fn enum_variant(enum_name: &str, variant: &str, value: Option<FuseValue>) -> Self {
        FuseValue::Enum(FuseEnum {
            enum_name: enum_name.into(),
            variant: variant.into(),
            value: value.map(Box::new),
        })
    }

    pub fn new_struct(type_name: &str, fields: Vec<(&str, FuseValue)>, del_fn: Option<&str>) -> Self {
        FuseValue::Struct(FuseStruct {
            type_name: type_name.into(),
            fields: fields.into_iter().map(|(k, v)| (k.to_string(), v)).collect(),
            del_fn: del_fn.map(|s| s.to_string()),
        })
    }

    // ── Accessors ────────────────────────────────────────────────────

    pub fn as_int(&self) -> i64 {
        match self { FuseValue::Int(v) => *v, _ => panic!("expected Int, got {}", self.type_name()) }
    }
    pub fn as_float(&self) -> f64 {
        match self { FuseValue::Float(v) => *v, _ => panic!("expected Float, got {}", self.type_name()) }
    }
    pub fn as_bool(&self) -> bool {
        match self { FuseValue::Bool(v) => *v, _ => panic!("expected Bool, got {}", self.type_name()) }
    }
    pub fn as_str(&self) -> &str {
        match self { FuseValue::Str(v) => v, _ => panic!("expected Str, got {}", self.type_name()) }
    }
    pub fn as_list(&self) -> &Vec<FuseValue> {
        match self { FuseValue::List(v) => v, _ => panic!("expected List, got {}", self.type_name()) }
    }
    pub fn as_list_mut(&mut self) -> &mut Vec<FuseValue> {
        match self { FuseValue::List(v) => v, _ => panic!("expected List, got {}", self.type_name()) }
    }

    pub fn field(&self, name: &str) -> FuseValue {
        match self {
            FuseValue::Struct(s) => {
                for (k, v) in &s.fields {
                    if k == name { return v.clone(); }
                }
                panic!("no field '{name}' on {}", s.type_name)
            }
            _ => panic!("field access on non-struct: {}", self.type_name()),
        }
    }

    pub fn set_field(&mut self, name: &str, value: FuseValue) {
        match self {
            FuseValue::Struct(s) => {
                for (k, v) in &mut s.fields {
                    if k == name { *v = value; return; }
                }
                panic!("no field '{name}' on {}", s.type_name)
            }
            _ => panic!("set_field on non-struct"),
        }
    }

    pub fn type_name(&self) -> &str {
        match self {
            FuseValue::Int(_) => "Int",
            FuseValue::Float(_) => "Float",
            FuseValue::Bool(_) => "Bool",
            FuseValue::Str(_) => "String",
            FuseValue::Unit => "Unit",
            FuseValue::List(_) => "List",
            FuseValue::Struct(s) => &s.type_name,
            FuseValue::Enum(e) => &e.enum_name,
            FuseValue::Fn(f) => "Fn",
            FuseValue::Lambda(_) => "Lambda",
        }
    }

    pub fn is_truthy(&self) -> bool {
        match self {
            FuseValue::Bool(b) => *b,
            FuseValue::Int(n) => *n != 0,
            FuseValue::Unit => false,
            FuseValue::Enum(e) if e.enum_name == "Option" && e.variant == "None" => false,
            _ => true,
        }
    }

    // ── Enum helpers ─────────────────────────────────────────────────

    pub fn is_ok(&self) -> bool {
        matches!(self, FuseValue::Enum(e) if e.enum_name == "Result" && e.variant == "Ok")
    }
    pub fn is_err(&self) -> bool {
        matches!(self, FuseValue::Enum(e) if e.enum_name == "Result" && e.variant == "Err")
    }
    pub fn is_some(&self) -> bool {
        matches!(self, FuseValue::Enum(e) if e.enum_name == "Option" && e.variant == "Some")
    }
    pub fn is_none(&self) -> bool {
        matches!(self, FuseValue::Enum(e) if e.enum_name == "Option" && e.variant == "None")
    }

    pub fn unwrap_enum_value(&self) -> FuseValue {
        match self {
            FuseValue::Enum(e) => match &e.value {
                Some(v) => *v.clone(),
                None => FuseValue::Unit,
            },
            _ => panic!("unwrap_enum_value on non-enum"),
        }
    }

    pub fn enum_variant_name(&self) -> &str {
        match self {
            FuseValue::Enum(e) => &e.variant,
            _ => panic!("enum_variant_name on non-enum"),
        }
    }

    pub fn enum_name(&self) -> &str {
        match self {
            FuseValue::Enum(e) => &e.enum_name,
            _ => panic!("enum_name on non-enum"),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Display / formatting
// ═══════════════════════════════════════════════════════════════════════

impl fmt::Display for FuseValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FuseValue::Int(v) => write!(f, "{v}"),
            FuseValue::Float(v) => {
                if *v == (*v as i64) as f64 && !v.is_nan() && v.is_finite() {
                    write!(f, "{v:.1}")
                } else {
                    write!(f, "{v}")
                }
            }
            FuseValue::Bool(v) => write!(f, "{}", if *v { "true" } else { "false" }),
            FuseValue::Str(v) => write!(f, "{v}"),
            FuseValue::Unit => write!(f, "()"),
            FuseValue::List(elems) => {
                write!(f, "[")?;
                for (i, e) in elems.iter().enumerate() {
                    if i > 0 { write!(f, ", ")?; }
                    write!(f, "{e}")?;
                }
                write!(f, "]")
            }
            FuseValue::Struct(s) => {
                write!(f, "{}(", s.type_name)?;
                for (i, (_, v)) in s.fields.iter().enumerate() {
                    if i > 0 { write!(f, ", ")?; }
                    write!(f, "{v}")?;
                }
                write!(f, ")")
            }
            FuseValue::Enum(e) => {
                if let Some(v) = &e.value {
                    write!(f, "{}({v})", e.variant)
                } else {
                    write!(f, "{}", e.variant)
                }
            }
            FuseValue::Fn(func) => write!(f, "<fn {}>", func.name),
            FuseValue::Lambda(_) => write!(f, "<lambda>"),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Arithmetic and comparison operators
// ═══════════════════════════════════════════════════════════════════════

impl FuseValue {
    pub fn add(&self, other: &FuseValue) -> FuseValue {
        match (self, other) {
            (FuseValue::Int(a), FuseValue::Int(b)) => FuseValue::Int(a + b),
            (FuseValue::Float(a), FuseValue::Float(b)) => FuseValue::Float(a + b),
            (FuseValue::Str(a), FuseValue::Str(b)) => FuseValue::Str(format!("{a}{b}")),
            _ => panic!("cannot add {} and {}", self.type_name(), other.type_name()),
        }
    }
    pub fn sub(&self, other: &FuseValue) -> FuseValue {
        match (self, other) {
            (FuseValue::Int(a), FuseValue::Int(b)) => FuseValue::Int(a - b),
            (FuseValue::Float(a), FuseValue::Float(b)) => FuseValue::Float(a - b),
            _ => panic!("cannot sub"),
        }
    }
    pub fn mul(&self, other: &FuseValue) -> FuseValue {
        match (self, other) {
            (FuseValue::Int(a), FuseValue::Int(b)) => FuseValue::Int(a * b),
            (FuseValue::Float(a), FuseValue::Float(b)) => FuseValue::Float(a * b),
            _ => panic!("cannot mul"),
        }
    }
    pub fn div(&self, other: &FuseValue) -> FuseValue {
        match (self, other) {
            (FuseValue::Int(a), FuseValue::Int(b)) => {
                if *b == 0 { panic!("division by zero"); }
                // Python-style floor division
                FuseValue::Int(a.div_euclid(*b))
            }
            (FuseValue::Float(a), FuseValue::Float(b)) => FuseValue::Float(a / b),
            _ => panic!("cannot div"),
        }
    }
    pub fn modulo(&self, other: &FuseValue) -> FuseValue {
        match (self, other) {
            (FuseValue::Int(a), FuseValue::Int(b)) => FuseValue::Int(a.rem_euclid(*b)),
            (FuseValue::Float(a), FuseValue::Float(b)) => FuseValue::Float(a % b),
            _ => panic!("cannot mod"),
        }
    }
    pub fn neg(&self) -> FuseValue {
        match self {
            FuseValue::Int(a) => FuseValue::Int(-a),
            FuseValue::Float(a) => FuseValue::Float(-a),
            _ => panic!("cannot negate"),
        }
    }
    pub fn eq(&self, other: &FuseValue) -> FuseValue {
        FuseValue::Bool(self.fuse_eq(other))
    }
    pub fn ne(&self, other: &FuseValue) -> FuseValue {
        FuseValue::Bool(!self.fuse_eq(other))
    }
    pub fn lt(&self, other: &FuseValue) -> FuseValue {
        match (self, other) {
            (FuseValue::Int(a), FuseValue::Int(b)) => FuseValue::Bool(a < b),
            (FuseValue::Float(a), FuseValue::Float(b)) => FuseValue::Bool(a < b),
            _ => panic!("cannot compare"),
        }
    }
    pub fn gt(&self, other: &FuseValue) -> FuseValue {
        match (self, other) {
            (FuseValue::Int(a), FuseValue::Int(b)) => FuseValue::Bool(a > b),
            (FuseValue::Float(a), FuseValue::Float(b)) => FuseValue::Bool(a > b),
            _ => panic!("cannot compare"),
        }
    }
    pub fn le(&self, other: &FuseValue) -> FuseValue {
        match (self, other) {
            (FuseValue::Int(a), FuseValue::Int(b)) => FuseValue::Bool(a <= b),
            (FuseValue::Float(a), FuseValue::Float(b)) => FuseValue::Bool(a <= b),
            _ => panic!("cannot compare"),
        }
    }
    pub fn ge(&self, other: &FuseValue) -> FuseValue {
        match (self, other) {
            (FuseValue::Int(a), FuseValue::Int(b)) => FuseValue::Bool(a >= b),
            (FuseValue::Float(a), FuseValue::Float(b)) => FuseValue::Bool(a >= b),
            _ => panic!("cannot compare"),
        }
    }

    fn fuse_eq(&self, other: &FuseValue) -> bool {
        match (self, other) {
            (FuseValue::Int(a), FuseValue::Int(b)) => a == b,
            (FuseValue::Float(a), FuseValue::Float(b)) => a == b,
            (FuseValue::Bool(a), FuseValue::Bool(b)) => a == b,
            (FuseValue::Str(a), FuseValue::Str(b)) => a == b,
            (FuseValue::Unit, FuseValue::Unit) => true,
            (FuseValue::Struct(a), FuseValue::Struct(b)) => {
                a.type_name == b.type_name && a.fields.len() == b.fields.len()
                    && a.fields.iter().zip(b.fields.iter())
                        .all(|((_, va), (_, vb))| va.fuse_eq(vb))
            }
            (FuseValue::Enum(a), FuseValue::Enum(b)) => {
                a.enum_name == b.enum_name && a.variant == b.variant
                    && match (&a.value, &b.value) {
                        (Some(va), Some(vb)) => va.fuse_eq(vb),
                        (None, None) => true,
                        _ => false,
                    }
            }
            _ => false,
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Early return (? operator support)
// ═══════════════════════════════════════════════════════════════════════

/// Returned from a Fuse function when ? encounters an Err/None.
#[derive(Clone)]
pub struct EarlyReturn {
    pub value: FuseValue,
}
