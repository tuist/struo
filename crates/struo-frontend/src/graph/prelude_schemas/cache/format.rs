use serde::ser::SerializeSeq;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::graph::{
    AttrSchema, Capability, DepSchema, SourceReference, TargetKindExample, TargetKindExampleSource,
    TargetKindSchema, ToolRequirement,
};
use crate::native_project::NativeProjectSchema;

use super::PreludeExports;

// Public schema JSON deliberately omits resolvers and example locations.
#[derive(Serialize, Deserialize)]
#[serde(remote = "TargetKindSchema")]
struct Schema {
    kind: String,
    docs: String,
    attrs: Vec<AttrSchema>,
    has_resolver: bool,
    #[serde(default)]
    accepts_ready_dependencies: bool,
    deps: Vec<DepSchema>,
    providers: Vec<String>,
    capabilities: Vec<Capability>,
    tools: Vec<ToolRequirement>,
    source_references: Vec<SourceReference>,
    #[serde(
        serialize_with = "serialize_examples",
        deserialize_with = "deserialize_examples"
    )]
    examples: Vec<TargetKindExample>,
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "TargetKindExample")]
struct Example {
    name: String,
    slug: String,
    use_when: String,
    platforms: Vec<String>,
    source: TargetKindExampleSource,
}

#[derive(Serialize)]
struct ExampleRef<'a>(#[serde(with = "Example")] &'a TargetKindExample);

#[derive(Deserialize)]
struct OwnedExample(#[serde(with = "Example")] TargetKindExample);

fn serialize_examples<S: Serializer>(
    examples: &[TargetKindExample],
    serializer: S,
) -> Result<S::Ok, S::Error> {
    let mut sequence = serializer.serialize_seq(Some(examples.len()))?;
    for example in examples {
        sequence.serialize_element(&ExampleRef(example))?;
    }
    sequence.end()
}

fn deserialize_examples<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<TargetKindExample>, D::Error> {
    Ok(Vec::<OwnedExample>::deserialize(deserializer)?
        .into_iter()
        .map(|example| example.0)
        .collect())
}

#[derive(Serialize)]
struct SchemaRef<'a>(#[serde(with = "Schema")] &'a TargetKindSchema);

#[derive(Deserialize)]
struct OwnedSchema(#[serde(with = "Schema")] TargetKindSchema);

#[derive(Serialize)]
pub(super) struct RecordRef<'a> {
    built_in_target_kinds: Option<Vec<SchemaRef<'a>>>,
    target_kinds: Vec<SchemaRef<'a>>,
    native_projects: &'a [NativeProjectSchema],
}

impl<'a> RecordRef<'a> {
    pub(super) fn new(exports: &'a PreludeExports) -> Option<Self> {
        Some(Self {
            built_in_target_kinds: match &exports.built_in_target_kinds {
                Some(schemas) => Some(schemas.as_ref().ok()?.iter().map(SchemaRef).collect()),
                None => None,
            },
            target_kinds: exports
                .target_kinds
                .as_ref()
                .ok()?
                .iter()
                .map(SchemaRef)
                .collect(),
            native_projects: exports.native_projects.as_ref().ok()?,
        })
    }
}

#[derive(Deserialize)]
pub(super) struct Record {
    built_in_target_kinds: Option<Vec<OwnedSchema>>,
    target_kinds: Vec<OwnedSchema>,
    native_projects: Vec<NativeProjectSchema>,
}

impl From<Record> for PreludeExports {
    fn from(record: Record) -> Self {
        Self {
            built_in_target_kinds: record
                .built_in_target_kinds
                .map(|schemas| Ok(schemas.into_iter().map(|schema| schema.0).collect())),
            target_kinds: Ok(record
                .target_kinds
                .into_iter()
                .map(|schema| schema.0)
                .collect()),
            native_projects: Ok(record.native_projects),
        }
    }
}
