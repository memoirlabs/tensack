use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrimitiveType {
    Id,
    Text,
    Int,
    Float,
    Bool,
}

impl PrimitiveType {
    pub fn matches_value(&self, value: &SackValue) -> bool {
        matches!(
            (self, value),
            (Self::Id, SackValue::Id(_))
                | (Self::Text, SackValue::Text(_))
                | (Self::Int, SackValue::Int(_))
                | (Self::Float, SackValue::Float(_))
                | (Self::Bool, SackValue::Bool(_))
        )
    }
}

impl fmt::Display for PrimitiveType {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Id => write!(formatter, "id"),
            Self::Text => write!(formatter, "text"),
            Self::Int => write!(formatter, "int"),
            Self::Float => write!(formatter, "float"),
            Self::Bool => write!(formatter, "bool"),
        }
    }
}

impl From<&str> for PrimitiveType {
    fn from(value: &str) -> Self {
        match value {
            "id" => Self::Id,
            "text" => Self::Text,
            "int" => Self::Int,
            "float" => Self::Float,
            "bool" => Self::Bool,
            _ => Self::Text,
        }
    }
}

impl From<PrimitiveType> for &'static str {
    fn from(value: PrimitiveType) -> Self {
        match value {
            PrimitiveType::Id => "id",
            PrimitiveType::Text => "text",
            PrimitiveType::Int => "int",
            PrimitiveType::Float => "float",
            PrimitiveType::Bool => "bool",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum SackValue {
    Id(String),
    Text(String),
    Int(i64),
    Float(f64),
    Bool(bool),
}

impl SackValue {
    pub fn value_type(&self) -> PrimitiveType {
        match self {
            Self::Id(_) => PrimitiveType::Id,
            Self::Text(_) => PrimitiveType::Text,
            Self::Int(_) => PrimitiveType::Int,
            Self::Float(_) => PrimitiveType::Float,
            Self::Bool(_) => PrimitiveType::Bool,
        }
    }
}

impl From<String> for SackValue {
    fn from(value: String) -> Self {
        Self::Text(value)
    }
}

impl From<&str> for SackValue {
    fn from(value: &str) -> Self {
        Self::Text(value.to_owned())
    }
}

impl From<i64> for SackValue {
    fn from(value: i64) -> Self {
        Self::Int(value)
    }
}

impl From<i32> for SackValue {
    fn from(value: i32) -> Self {
        Self::Int(value as i64)
    }
}

impl From<f64> for SackValue {
    fn from(value: f64) -> Self {
        Self::Float(value)
    }
}

impl From<bool> for SackValue {
    fn from(value: bool) -> Self {
        Self::Bool(value)
    }
}
