use serde_json::Value as JsonValue;
use starlark::environment::Module;
use starlark::eval::Evaluator;
use starlark::values::dict::{AllocDict, DictRef};
use starlark::values::list::{AllocList, ListRef};
use starlark::values::{OwnedFrozenValue, Value};

use super::super::transitive_set;
use super::super::values::{json_to_value, resolve_provider_reference};

#[derive(Debug)]
pub(super) struct Template {
    value: OwnedFrozenValue,
    containers: Containers,
}

impl Template {
    pub(super) fn new(json: &JsonValue) -> Self {
        let value = Module::with_temp_heap(|module| {
            module.set("provider", json_to_value(module.heap(), json));
            module
                .freeze()
                .expect("JSON values can be frozen")
                .get("provider")
                .expect("provider was bound before freezing")
        });
        Self {
            value,
            containers: Containers::new(json),
        }
    }

    pub(super) fn materialize<'v>(
        &self,
        eval: &Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<Value<'v>> {
        let value = eval.heap().access_owned_frozen_value(&self.value);
        self.containers.copy(eval, value)
    }
}

#[derive(Debug)]
enum Containers {
    Scalar,
    List(Vec<(usize, Self)>),
    Dict(Vec<Self>),
    Reference,
}

impl Containers {
    fn new(json: &JsonValue) -> Self {
        match json {
            JsonValue::Array(items) => Self::List(
                items
                    .iter()
                    .enumerate()
                    .filter(|(_, item)| item.is_array() || item.is_object())
                    .map(|(index, item)| (index, Self::new(item)))
                    .collect(),
            ),
            JsonValue::Object(entries) if transitive_set::reference(entries).is_some() => {
                Self::Reference
            }
            JsonValue::Object(entries) => Self::Dict(entries.values().map(Self::new).collect()),
            _ => Self::Scalar,
        }
    }

    fn copy<'v>(
        &self,
        eval: &Evaluator<'v, '_, '_>,
        value: Value<'v>,
    ) -> anyhow::Result<Value<'v>> {
        let heap = eval.heap();
        Ok(match self {
            Self::Scalar => value,
            Self::List(children) => {
                let items = ListRef::from_value(value).expect("template is a list");
                if children.is_empty() {
                    heap.alloc(AllocList(items.iter()))
                } else {
                    let mut values = items.content().to_vec();
                    for (index, child) in children {
                        values[*index] = child.copy(eval, values[*index])?;
                    }
                    heap.alloc(values)
                }
            }
            Self::Dict(children) => {
                let entries = DictRef::from_value(value).expect("template is a dict");
                let pairs = entries
                    .iter()
                    .zip(children)
                    .map(|((key, value), child)| Ok((key, child.copy(eval, value)?)))
                    .collect::<anyhow::Result<Vec<_>>>()?;
                heap.alloc(AllocDict(pairs))
            }
            Self::Reference => {
                let entries = DictRef::from_value(value).expect("reference is a dict");
                let digest = entries
                    .get_str(transitive_set::REFERENCE_KEY)
                    .and_then(Value::unpack_str)
                    .expect("reference contains a string digest");
                resolve_provider_reference(eval, digest)?
            }
        })
    }
}
