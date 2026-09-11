//! The server's OpenAPI document, built from the engine's own types.
//!
//! # Why it is generated
//!
//! A hand-written spec is a second description of the API that nothing keeps
//! honest: it is correct on the day it is written and quietly wrong from the
//! first handler change onward. Everything in this repo that describes a shape
//! — the UI's TypeScript types, the settings form, the profile editors — is
//! derived from the Rust types by `schemars`, and this is no different.
//!
//! So **bodies come from the types** ([`schemars::schema_for!`]), and only the
//! *route table* below is written by hand. That split is deliberate: the bodies
//! are where drift actually hurts and where there are hundreds of fields, while
//! the routes are a dozen lines that change when someone edits the router two
//! files away and will notice.
//!
//! # Translating schemars into OpenAPI
//!
//! JSON Schema and OpenAPI 3.1 agree on almost everything, with one wrinkle:
//! `schemars` puts subschemas in `$defs` and points at them with
//! `#/$defs/Name`, while OpenAPI expects them in `components/schemas` and
//! `#/components/schemas/Name`. [`hoist`] moves them and rewrites every `$ref`
//! in the tree. Nothing else is transformed — the field-level `description`s,
//! `default`s, ranges and `x-` extensions the engine already emits carry
//! straight through, which is why the rendered reference reads the same way the
//! settings panel does.

use schemars::schema_for;
use serde_json::{json, Map, Value};

/// One documented operation.
struct Route {
    /// Path template, in OpenAPI form (`/api/download/{request_uuid}`).
    path: &'static str,
    /// Lowercase HTTP method.
    method: &'static str,
    /// One-line summary.
    summary: &'static str,
    /// Longer prose. Written for a person integrating against the server.
    description: &'static str,
    /// Path/query parameters, as `(name, r#in, description)`.
    params: &'static [(&'static str, &'static str, &'static str)],
    /// Request body schema name in `components/schemas`, if any.
    request: Option<&'static str>,
    /// `(status, description, schema name or None for a non-JSON body)`.
    responses: &'static [(&'static str, &'static str, Option<&'static str>)],
}

/// Every route the server exposes, in the order the router registers them.
///
/// Keep this in step with [`super::mod`]'s route table — `routes_match_router`
/// in the tests below asserts the count so a new endpoint cannot land
/// undocumented without something going red.
const ROUTES: &[Route] = &[
    Route {
        path: "/api/upload",
        method: "post",
        summary: "Upload a model into a workplate",
        description: "Multipart upload. Send the model as `file`; to add it to an \
                      existing plate rather than starting a new one, send that plate's \
                      `ruuid` **before** the file field — multipart fields stream in \
                      order, and the server needs the plate before the bytes. Returns \
                      the workplate uuid and the file uuid the slice protocol refers to.",
        params: &[],
        request: None,
        responses: &[
            (
                "200",
                "The workplate and the file it now holds",
                Some("UploadResponse"),
            ),
            (
                "400",
                "No file in the request, or an unusable `ruuid`",
                None,
            ),
        ],
    },
    Route {
        path: "/api/download/{request_uuid}",
        method: "get",
        summary: "Download a workplate's G-code",
        description: "Streams the G-code produced by the most recent slice of this \
                      plate. 404 until the plate has been sliced at least once.",
        params: &[("request_uuid", "path", "Workplate uuid")],
        request: None,
        responses: &[
            ("200", "The G-code file", None),
            ("404", "No G-code has been generated for this plate", None),
        ],
    },
    Route {
        path: "/api/request/{request_uuid}",
        method: "get",
        summary: "Read a workplate's files and status",
        description: "Everything the server knows about a plate: its lifecycle status \
                      and the files uploaded into it. This is what lets a browser \
                      rebuild a slice request after a reload.",
        params: &[("request_uuid", "path", "Workplate uuid")],
        request: None,
        responses: &[
            ("200", "Workplate metadata", Some("RequestMetaResponse")),
            ("404", "No such workplate", None),
        ],
    },
    Route {
        path: "/api/file/{file_uuid}",
        method: "get",
        summary: "Download an uploaded model",
        description: "Streams back the bytes of one uploaded model, with the extension \
                      it was stored under. Used by the viewer on a cold reload.",
        params: &[("file_uuid", "path", "File uuid from an upload's `ofids`")],
        request: None,
        responses: &[
            ("200", "The model file", None),
            ("404", "No such file", None),
        ],
    },
    Route {
        path: "/api/config",
        method: "get",
        summary: "Read the merged runtime configuration",
        description: "The configuration as the engine actually resolved it, after \
                      merging defaults, `slicer.toml` and the environment.",
        params: &[],
        request: None,
        responses: &[("200", "The resolved configuration", Some("ConfigResponse"))],
    },
    Route {
        path: "/api/config",
        method: "patch",
        summary: "Set one configuration key",
        description: "Writes a single dot-separated key through to `slicer.toml`. \
                      Returns the configuration as it stands afterwards.",
        params: &[],
        request: Some("PatchConfigRequest"),
        responses: &[
            (
                "200",
                "The configuration after the write",
                Some("ConfigResponse"),
            ),
            ("400", "Unknown key, or a value of the wrong type", None),
        ],
    },
    Route {
        path: "/api/profiles",
        method: "get",
        summary: "Read the profile library",
        description: "Every printer, filament, process and label this slicer holds. \
                      Each sliceable category always has at least one entry: a slice \
                      request names its profiles by id, so an empty category would \
                      leave nothing to resolve, and the built-in default is seeded \
                      rather than left missing.",
        params: &[],
        request: None,
        responses: &[("200", "The whole library", Some("ProfileLibrary"))],
    },
    Route {
        path: "/api/profiles/{kind}",
        method: "put",
        summary: "Replace one profile category",
        description: "Whole-category, last writer wins — send the full array for the \
                      category on any add, edit or delete. Returns the updated library, \
                      and nudges every open WebSocket session to refetch.",
        params: &[(
            "kind",
            "path",
            "`printers`, `filaments`, `processes` or `labels`",
        )],
        request: None,
        responses: &[
            ("200", "The library after the write", Some("ProfileLibrary")),
            ("400", "The array did not decode as that category", None),
            ("404", "Unknown category", None),
        ],
    },
    Route {
        path: "/api/profiles/export",
        method: "get",
        summary: "Download the profile library",
        description: "`format=bundle` (default) returns a ZIP with one TOML per \
                      profile; `format=toml` returns a single `profiles.toml`, the same \
                      file the command line reads. Credentials are stripped from both.",
        params: &[("format", "query", "`bundle` or `toml`")],
        request: None,
        responses: &[
            ("200", "The rendered export", None),
            ("400", "Unknown format", None),
        ],
    },
    Route {
        path: "/api/workplates/{request_uuid}",
        method: "get",
        summary: "Read a workplate's saved setup",
        description: "The plate as a saved document: which printer, filament and \
                      process it was set up with, the user's sparse setting diff, and \
                      where each object sits. References throughout — profile ids and \
                      file ids, never copies. A plate nobody configured answers with an \
                      empty document rather than a 404.",
        params: &[("request_uuid", "path", "Workplate uuid")],
        request: None,
        responses: &[
            ("200", "The saved setup", Some("WorkplateSetup")),
            ("400", "Not a uuid", None),
        ],
    },
    Route {
        path: "/api/workplates/{request_uuid}",
        method: "put",
        summary: "Replace a workplate's saved setup",
        description: "Whole-document, last writer wins. Saving a plate does not affect \
                      what a slice produces: a slice request carries the scene it is \
                      slicing in full, and this is only what the plate is restored from.",
        params: &[("request_uuid", "path", "Workplate uuid")],
        request: Some("WorkplateSetup"),
        responses: &[
            (
                "200",
                "The document as stored, with its save timestamp",
                Some("WorkplateSetup"),
            ),
            ("400", "Not a uuid, or the body did not decode", None),
        ],
    },
    Route {
        path: "/api/openapi.json",
        method: "get",
        summary: "This document",
        description: "The OpenAPI 3.1 description of this server, generated from the \
                      engine's Rust types on every request. Point a client generator at \
                      it and what comes out matches the build you are talking to.",
        params: &[],
        request: None,
        responses: &[("200", "The OpenAPI document", None)],
    },
    Route {
        path: "/api/docs",
        method: "get",
        summary: "This page",
        description: "A browsable reference rendered from `/api/openapi.json`. \
                      Self-contained — it needs no internet access, which matters on a \
                      self-hosted slicer that has none.",
        params: &[],
        request: None,
        responses: &[("200", "The reference page", None)],
    },
    Route {
        path: "/api/history",
        method: "delete",
        summary: "Drop all slicing history",
        description: "Deletes every workplate row, uploaded file and cached G-code. \
                      Saved workplate setups go with them, since they live on the \
                      plate's own row. Profiles are untouched.",
        params: &[],
        request: None,
        responses: &[("200", "How much was removed", None)],
    },
];

/// Move `schemars`' `$defs` into `components/schemas` and rewrite every `$ref`.
///
/// `schema_for!` returns a self-contained document: the root schema plus a
/// `$defs` map of everything it references. OpenAPI wants one shared component
/// map, so each type's `$defs` are merged into `components` and the root is
/// registered under `name`. Types that reference the same subschema simply
/// agree on it — `schemars` names them identically, and they *are* identical,
/// because they come from the same Rust type.
fn hoist(name: &str, mut schema: Value, components: &mut Map<String, Value>) {
    if let Some(defs) = schema
        .as_object_mut()
        .and_then(|m| m.remove("$defs"))
        .and_then(|d| match d {
            Value::Object(m) => Some(m),
            _ => None,
        })
    {
        for (def_name, def) in defs {
            let mut def = def;
            rewrite_refs(&mut def);
            components.entry(def_name).or_insert(def);
        }
    }
    // The `$schema` marker is a JSON Schema document header, meaningless on a
    // component and rejected by some OpenAPI tooling.
    if let Some(map) = schema.as_object_mut() {
        map.remove("$schema");
        map.remove("title");
    }
    rewrite_refs(&mut schema);
    components.insert(name.to_string(), schema);
}

/// Rewrite `#/$defs/X` to `#/components/schemas/X`, everywhere in the tree.
fn rewrite_refs(value: &mut Value) {
    match value {
        Value::Object(map) => {
            if let Some(Value::String(reference)) = map.get_mut("$ref") {
                if let Some(rest) = reference.strip_prefix("#/$defs/") {
                    *reference = format!("#/components/schemas/{rest}");
                }
            }
            for (_, v) in map.iter_mut() {
                rewrite_refs(v);
            }
        }
        Value::Array(items) => items.iter_mut().for_each(rewrite_refs),
        _ => {}
    }
}

/// Every schema the route table refers to, keyed by the name used there.
fn components() -> Map<String, Value> {
    let mut components = Map::new();
    let mut add = |name: &str, schema: schemars::Schema| {
        hoist(
            name,
            serde_json::to_value(schema).expect("schema serializes"),
            &mut components,
        );
    };

    add(
        "UploadResponse",
        schema_for!(super::handlers::UploadResponse),
    );
    add(
        "ConfigResponse",
        schema_for!(super::handlers::ConfigResponse),
    );
    add(
        "PatchConfigRequest",
        schema_for!(super::handlers::PatchConfigRequest),
    );
    add(
        "RequestMetaResponse",
        schema_for!(super::handlers::RequestMetaResponse),
    );
    add(
        "ProfileLibrary",
        schema_for!(crate::profiles::ProfileLibrary),
    );
    add(
        "WorkplateSetup",
        schema_for!(crate::workplate::WorkplateSetup),
    );
    add(
        "ClientMessage",
        schema_for!(crate::ws_protocol::ClientMessage),
    );
    add(
        "ServerMessage",
        schema_for!(crate::ws_protocol::ServerMessage),
    );
    components
}

/// Build the OpenAPI 3.1 document for this server.
pub fn document() -> Value {
    let mut paths: Map<String, Value> = Map::new();

    for route in ROUTES {
        let params: Vec<Value> = route
            .params
            .iter()
            .map(|(name, location, description)| {
                json!({
                    "name": name,
                    "in": location,
                    "required": *location == "path",
                    "description": description,
                    "schema": { "type": "string" },
                })
            })
            .collect();

        let responses: Map<String, Value> = route
            .responses
            .iter()
            .map(|(status, description, schema)| {
                let body = match schema {
                    Some(name) => json!({
                        "description": description,
                        "content": {
                            "application/json": {
                                "schema": { "$ref": format!("#/components/schemas/{name}") }
                            }
                        }
                    }),
                    None => json!({ "description": description }),
                };
                (status.to_string(), body)
            })
            .collect();

        let mut operation = json!({
            "summary": route.summary,
            "description": route.description,
            "responses": responses,
        });
        if !params.is_empty() {
            operation["parameters"] = Value::Array(params);
        }
        if let Some(name) = route.request {
            operation["requestBody"] = json!({
                "required": true,
                "content": {
                    "application/json": {
                        "schema": { "$ref": format!("#/components/schemas/{name}") }
                    }
                }
            });
        }

        paths
            .entry(route.path.to_string())
            .or_insert_with(|| json!({}))
            .as_object_mut()
            .expect("path item is an object")
            .insert(route.method.to_string(), operation);
    }

    json!({
        "openapi": "3.1.0",
        "info": {
            "title": "Cold Crabby slicer",
            "version": crate::version::VERSION,
            "description": "The HTTP surface of a running slicer. Slicing itself is not \
                            here: it runs over the WebSocket at `/ws`, because a slice \
                            streams progress and log lines for as long as it takes. The \
                            messages that socket speaks are documented as the \
                            `ClientMessage` and `ServerMessage` schemas below.\n\n\
                            Schemas on this page are generated from the engine's own \
                            Rust types, so they describe what this build actually \
                            accepts — not what a spec file last claimed.",
        },
        "paths": paths,
        "components": { "schemas": components() },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every documented route must name a schema that exists, or the reference
    /// renders as a dead link in the viewer.
    #[test]
    fn every_referenced_schema_exists() {
        let doc = document();
        let schemas = doc["components"]["schemas"].as_object().expect("schemas");
        for route in ROUTES {
            if let Some(name) = route.request {
                assert!(
                    schemas.contains_key(name),
                    "request schema '{name}' missing"
                );
            }
            for (_, _, schema) in route.responses {
                if let Some(name) = schema {
                    assert!(
                        schemas.contains_key(*name),
                        "response schema '{name}' missing"
                    );
                }
            }
        }
    }

    /// `$defs` is JSON Schema's spelling; leaving one behind means a `$ref`
    /// pointing at nothing once the document is read as OpenAPI.
    #[test]
    fn no_json_schema_refs_survive() {
        let doc = document().to_string();
        assert!(!doc.contains("#/$defs/"), "an unrewritten $ref survived");
        assert!(!doc.contains("\"$defs\""), "a $defs block survived");
    }

    /// The document is only useful if it covers the router. This is a count,
    /// not a proof — but it fails loudly when someone adds an endpoint.
    #[test]
    fn routes_match_router() {
        // `.route(` counts both spellings — one-line and the wrapped form
        // rustfmt produces for longer paths.
        let router = include_str!("mod.rs").matches(".route(").count();
        // `/ws` is a WebSocket upgrade, not an OpenAPI operation.
        assert_eq!(
            ROUTES.len(),
            router - 1,
            "the router has {router} routes (one being /ws) but {} are documented",
            ROUTES.len()
        );
    }

    #[test]
    fn the_document_is_openapi_3_1_and_names_this_build() {
        let doc = document();
        assert_eq!(doc["openapi"], "3.1.0");
        assert_eq!(doc["info"]["version"], crate::version::VERSION);
    }

    /// Field descriptions written on the Rust types have to survive, or the
    /// reference is a list of names with no explanation.
    #[test]
    fn descriptions_come_through_from_the_types() {
        let doc = document();
        let setup = &doc["components"]["schemas"]["WorkplateSetup"]["properties"];
        assert!(
            setup["overrides"]["description"]
                .as_str()
                .is_some_and(|d| d.contains("only the keys the user changed")),
            "the doc comment on WorkplateSetup::overrides did not carry through"
        );
    }
}
