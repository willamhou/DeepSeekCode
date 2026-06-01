use crate::model::protocol::ModelRequest;
use crate::util::json::{json_value_to_string, JsonValue};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PromptLayerRecord {
    pub name: String,
    pub text_sha256: String,
    pub bytes: usize,
    pub estimated_tokens: u64,
    pub cache_stable: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PromptLayerSnapshot {
    pub step: usize,
    pub layers: Vec<PromptLayerRecord>,
    pub total_bytes: usize,
    pub estimated_tokens: u64,
}

pub fn prompt_layers_for_request(step: usize, request: &ModelRequest) -> PromptLayerSnapshot {
    let mut layers = Vec::new();
    push_layer(&mut layers, "system_static", &request.system_prompt, true);
    push_layer(
        &mut layers,
        "tool_catalog",
        &request.available_tools.join("\n"),
        true,
    );
    push_layer(
        &mut layers,
        "workspace_profile",
        &workspace_profile_text(request),
        true,
    );
    push_layer(
        &mut layers,
        "task_context",
        &task_context_text(request),
        false,
    );
    push_layer(&mut layers, "user_task", &request.task, false);
    push_layer(
        &mut layers,
        "media_inputs",
        &media_inputs_text(request),
        false,
    );
    push_layer(&mut layers, "active_todos", &todos_text(request), false);
    push_layer(
        &mut layers,
        "append_only_turns",
        &append_only_turns_text(request),
        false,
    );
    push_layer(
        &mut layers,
        "volatile_scratch",
        &volatile_scratch_text(request),
        false,
    );

    let total_bytes = layers.iter().map(|layer| layer.bytes).sum();
    let estimated_tokens = layers
        .iter()
        .map(|layer| layer.estimated_tokens)
        .sum::<u64>();
    PromptLayerSnapshot {
        step,
        layers,
        total_bytes,
        estimated_tokens,
    }
}

pub fn prompt_layer_snapshot_to_json(snapshot: &PromptLayerSnapshot) -> JsonValue {
    JsonValue::Object(
        [
            (
                "step".to_string(),
                JsonValue::Number(snapshot.step.to_string()),
            ),
            (
                "total_bytes".to_string(),
                JsonValue::Number(snapshot.total_bytes.to_string()),
            ),
            (
                "estimated_tokens".to_string(),
                JsonValue::Number(snapshot.estimated_tokens.to_string()),
            ),
            (
                "layers".to_string(),
                JsonValue::Array(
                    snapshot
                        .layers
                        .iter()
                        .map(prompt_layer_record_to_json)
                        .collect(),
                ),
            ),
        ]
        .into_iter()
        .collect(),
    )
}

fn prompt_layer_record_to_json(layer: &PromptLayerRecord) -> JsonValue {
    JsonValue::Object(
        [
            ("name".to_string(), JsonValue::String(layer.name.clone())),
            (
                "text_sha256".to_string(),
                JsonValue::String(layer.text_sha256.clone()),
            ),
            (
                "bytes".to_string(),
                JsonValue::Number(layer.bytes.to_string()),
            ),
            (
                "estimated_tokens".to_string(),
                JsonValue::Number(layer.estimated_tokens.to_string()),
            ),
            (
                "cache_stable".to_string(),
                JsonValue::Bool(layer.cache_stable),
            ),
        ]
        .into_iter()
        .collect(),
    )
}

fn push_layer(layers: &mut Vec<PromptLayerRecord>, name: &str, text: &str, cache_stable: bool) {
    if text.trim().is_empty() {
        return;
    }
    let bytes = text.len();
    layers.push(PromptLayerRecord {
        name: name.to_string(),
        text_sha256: sha256_hex(text.as_bytes()),
        bytes,
        estimated_tokens: estimate_tokens(text),
        cache_stable,
    });
}

fn workspace_profile_text(request: &ModelRequest) -> String {
    let mut text = String::new();
    text.push_str("profile=");
    text.push_str(&request.profile_name);
    if !request.profile_hints.is_empty() {
        text.push_str("\nhints=");
        text.push_str(&request.profile_hints.join("\n"));
    }
    text
}

fn task_context_text(request: &ModelRequest) -> String {
    let mut text = String::new();
    if let Some(primary_file) = request.primary_file.as_deref() {
        text.push_str("primary_file=");
        text.push_str(primary_file);
    }
    if let Some(command) = request.suggested_test_command.as_deref() {
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str("suggested_test_command=");
        text.push_str(command);
    }
    text
}

fn media_inputs_text(request: &ModelRequest) -> String {
    request
        .image_inputs
        .iter()
        .map(|image| format!("{} {}", image.media_type, image.path))
        .collect::<Vec<_>>()
        .join("\n")
}

fn todos_text(request: &ModelRequest) -> String {
    request
        .todos
        .iter()
        .map(|todo| {
            format!(
                "{}\t{}\t{}",
                todo.status.label(),
                todo.content,
                todo.active_form
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn append_only_turns_text(request: &ModelRequest) -> String {
    let observations = request
        .observations
        .iter()
        .map(|observation| {
            format!(
                "{}\t{}\t{}",
                observation.tool_name,
                match observation.status {
                    crate::model::protocol::ObservationStatus::Ok => "ok",
                    crate::model::protocol::ObservationStatus::Failed => "failed",
                },
                observation.summary
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let recent_steps = request.recent_steps.join("\n");
    match (observations.is_empty(), recent_steps.is_empty()) {
        (true, true) => String::new(),
        (false, true) => observations,
        (true, false) => recent_steps,
        (false, false) => format!("{observations}\n{recent_steps}"),
    }
}

fn volatile_scratch_text(request: &ModelRequest) -> String {
    let mut text = String::new();
    if request.planning_mode {
        text.push_str("planning_mode_active\n");
    }
    text
}

fn estimate_tokens(text: &str) -> u64 {
    if text.is_empty() {
        return 0;
    }
    (text.len() as u64).div_ceil(4).max(1)
}

fn sha256_hex(input: &[u8]) -> String {
    const H0: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];

    let bit_len = (input.len() as u64).wrapping_mul(8);
    let mut message = input.to_vec();
    message.push(0x80);
    while (message.len() % 64) != 56 {
        message.push(0);
    }
    message.extend_from_slice(&bit_len.to_be_bytes());

    let mut hash = H0;
    for chunk in message.chunks_exact(64) {
        let mut w = [0_u32; 64];
        for (index, word) in w.iter_mut().take(16).enumerate() {
            let offset = index * 4;
            *word = u32::from_be_bytes([
                chunk[offset],
                chunk[offset + 1],
                chunk[offset + 2],
                chunk[offset + 3],
            ]);
        }
        for index in 16..64 {
            let s0 = w[index - 15].rotate_right(7)
                ^ w[index - 15].rotate_right(18)
                ^ (w[index - 15] >> 3);
            let s1 = w[index - 2].rotate_right(17)
                ^ w[index - 2].rotate_right(19)
                ^ (w[index - 2] >> 10);
            w[index] = w[index - 16]
                .wrapping_add(s0)
                .wrapping_add(w[index - 7])
                .wrapping_add(s1);
        }

        let mut a = hash[0];
        let mut b = hash[1];
        let mut c = hash[2];
        let mut d = hash[3];
        let mut e = hash[4];
        let mut f = hash[5];
        let mut g = hash[6];
        let mut h = hash[7];

        for index in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = h
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[index])
                .wrapping_add(w[index]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);
            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }

        hash[0] = hash[0].wrapping_add(a);
        hash[1] = hash[1].wrapping_add(b);
        hash[2] = hash[2].wrapping_add(c);
        hash[3] = hash[3].wrapping_add(d);
        hash[4] = hash[4].wrapping_add(e);
        hash[5] = hash[5].wrapping_add(f);
        hash[6] = hash[6].wrapping_add(g);
        hash[7] = hash[7].wrapping_add(h);
    }

    hash.iter()
        .map(|word| format!("{word:08x}"))
        .collect::<Vec<_>>()
        .join("")
}

pub fn prompt_layer_snapshots_to_json(snapshots: &[PromptLayerSnapshot]) -> JsonValue {
    JsonValue::Array(
        snapshots
            .iter()
            .map(prompt_layer_snapshot_to_json)
            .collect(),
    )
}

pub fn prompt_layer_digest(snapshots: &[PromptLayerSnapshot]) -> String {
    sha256_hex(json_value_to_string(&prompt_layer_snapshots_to_json(snapshots)).as_bytes())
}

pub fn prompt_layers_event_payload(
    turn_id: &str,
    usage_id: &str,
    snapshots: &[PromptLayerSnapshot],
) -> JsonValue {
    JsonValue::Object(
        [
            (
                "type".to_string(),
                JsonValue::String("prompt_layers_recorded".to_string()),
            ),
            (
                "turn_id".to_string(),
                JsonValue::String(turn_id.to_string()),
            ),
            (
                "usage_id".to_string(),
                JsonValue::String(usage_id.to_string()),
            ),
            (
                "snapshot_count".to_string(),
                JsonValue::Number(snapshots.len().to_string()),
            ),
            (
                "digest".to_string(),
                JsonValue::String(prompt_layer_digest(snapshots)),
            ),
            (
                "snapshots".to_string(),
                prompt_layer_snapshots_to_json(snapshots),
            ),
        ]
        .into_iter()
        .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::protocol::{ImageInput, ModelRequest};
    use crate::util::json::json_as_string;

    #[test]
    fn sha256_matches_known_vector() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn prompt_layers_include_stable_and_volatile_parts() {
        let request = ModelRequest {
            system_prompt: "system".to_string(),
            task: "task".to_string(),
            profile_name: "rust".to_string(),
            profile_hints: vec!["hint".to_string()],
            primary_file: Some("src/lib.rs".to_string()),
            suggested_test_command: Some("cargo test".to_string()),
            available_tools: vec!["read_file".to_string()],
            observations: vec![crate::model::protocol::Observation::ok("read_file", "ok")],
            todos: Vec::new(),
            planning_mode: true,
            recent_steps: vec!["assistant planned".to_string()],
            image_inputs: vec![ImageInput {
                path: "diagram.png".to_string(),
                media_type: "image/png".to_string(),
                data_base64: "AAAA".to_string(),
            }],
        };

        let snapshot = prompt_layers_for_request(1, &request);
        assert!(snapshot.estimated_tokens > 0);
        assert!(snapshot
            .layers
            .iter()
            .any(|layer| layer.name == "system_static" && layer.cache_stable));
        assert!(snapshot
            .layers
            .iter()
            .any(|layer| layer.name == "append_only_turns" && !layer.cache_stable));
        assert!(snapshot
            .layers
            .iter()
            .any(|layer| layer.name == "task_context" && !layer.cache_stable));
        assert!(snapshot
            .layers
            .iter()
            .any(|layer| layer.name == "user_task" && !layer.cache_stable));
        assert!(snapshot
            .layers
            .iter()
            .any(|layer| layer.name == "media_inputs" && !layer.cache_stable));
    }

    #[test]
    fn volatile_task_context_changes_do_not_break_stable_prefix() {
        let first = ModelRequest {
            system_prompt: "system".to_string(),
            task: "inspect first task".to_string(),
            profile_name: "rust".to_string(),
            profile_hints: vec!["hint".to_string()],
            primary_file: Some("src/lib.rs".to_string()),
            suggested_test_command: Some("cargo test".to_string()),
            available_tools: vec!["read_file".to_string()],
            observations: Vec::new(),
            todos: Vec::new(),
            planning_mode: false,
            recent_steps: Vec::new(),
            image_inputs: vec![ImageInput {
                path: "first.png".to_string(),
                media_type: "image/png".to_string(),
                data_base64: "AAAA".to_string(),
            }],
        };
        let mut second = first.clone();
        second.task = "inspect second task".to_string();
        second.primary_file = Some("src/main.rs".to_string());
        second.suggested_test_command = Some("cargo test --lib".to_string());
        second.image_inputs = vec![ImageInput {
            path: "second.png".to_string(),
            media_type: "image/png".to_string(),
            data_base64: "BBBB".to_string(),
        }];

        let first_snapshot = prompt_layers_for_request(1, &first);
        let second_snapshot = prompt_layers_for_request(2, &second);
        let mut stable_hash_changes = 0;
        for first_layer in first_snapshot
            .layers
            .iter()
            .filter(|layer| layer.cache_stable)
        {
            let second_layer = second_snapshot
                .layers
                .iter()
                .find(|layer| layer.name == first_layer.name)
                .expect("stable layer should still be present");
            if first_layer.text_sha256 != second_layer.text_sha256 {
                stable_hash_changes += 1;
            }
        }

        assert_eq!(stable_hash_changes, 0);
    }

    #[test]
    fn prompt_layers_event_payload_links_turn_usage_and_digest() {
        let snapshot = PromptLayerSnapshot {
            step: 1,
            layers: vec![PromptLayerRecord {
                name: "system_static".to_string(),
                text_sha256: "abc123".to_string(),
                bytes: 12,
                estimated_tokens: 3,
                cache_stable: true,
            }],
            total_bytes: 12,
            estimated_tokens: 3,
        };

        let JsonValue::Object(root) = prompt_layers_event_payload("turn-1", "usage-1", &[snapshot])
        else {
            panic!("payload should be object");
        };
        assert_eq!(
            root.get("type").and_then(json_as_string),
            Some("prompt_layers_recorded")
        );
        assert_eq!(root.get("turn_id").and_then(json_as_string), Some("turn-1"));
        assert_eq!(
            root.get("usage_id").and_then(json_as_string),
            Some("usage-1")
        );
        assert_eq!(
            root.get("snapshot_count").and_then(|value| match value {
                JsonValue::Number(raw) => raw.parse::<u64>().ok(),
                _ => None,
            }),
            Some(1)
        );
        assert!(root.get("digest").and_then(json_as_string).is_some());
        assert!(matches!(root.get("snapshots"), Some(JsonValue::Array(_))));
    }

    #[test]
    fn prompt_layer_payload_omits_raw_prompt_text() {
        let request = ModelRequest {
            system_prompt: "system prompt text that must stay hashed".to_string(),
            task: "user task text that must stay hashed".to_string(),
            profile_name: "rust".to_string(),
            profile_hints: vec!["profile hint text that must stay hashed".to_string()],
            primary_file: Some("src/private.rs".to_string()),
            suggested_test_command: Some("cargo test private".to_string()),
            available_tools: vec!["read_file".to_string()],
            observations: vec![crate::model::protocol::Observation::ok(
                "read_file",
                "observation summary that must stay hashed",
            )],
            todos: Vec::new(),
            planning_mode: false,
            recent_steps: vec!["assistant step that must stay hashed".to_string()],
            image_inputs: Vec::new(),
        };

        let snapshot = prompt_layers_for_request(1, &request);
        let payload = json_value_to_string(&prompt_layers_event_payload(
            "turn-1",
            "usage-1",
            &[snapshot],
        ));

        for raw in [
            "system prompt text that must stay hashed",
            "user task text that must stay hashed",
            "profile hint text that must stay hashed",
            "src/private.rs",
            "cargo test private",
            "observation summary that must stay hashed",
            "assistant step that must stay hashed",
        ] {
            assert!(
                !payload.contains(raw),
                "prompt-layer payload leaked raw text: {raw}"
            );
        }
        assert!(payload.contains("text_sha256"));
        assert!(payload.contains("estimated_tokens"));
    }
}
