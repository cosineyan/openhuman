use super::*;

fn make_channel() -> LarkChannel {
    LarkChannel::new(
        "cli_test_app_id".into(),
        "test_app_secret".into(),
        "test_verification_token".into(),
        None,
        vec!["ou_testuser123".into()],
    )
}

#[test]
fn lark_channel_name() {
    let ch = make_channel();
    assert_eq!(ch.name(), "lark");
}

#[test]
fn lark_ws_activity_refreshes_heartbeat_watchdog() {
    assert!(should_refresh_last_recv(&WsMsg::Binary(
        vec![1, 2, 3].into()
    )));
    assert!(should_refresh_last_recv(&WsMsg::Ping(vec![9, 9].into())));
    assert!(should_refresh_last_recv(&WsMsg::Pong(vec![8, 8].into())));
}

#[test]
fn lark_ws_non_activity_frames_do_not_refresh_heartbeat_watchdog() {
    assert!(!should_refresh_last_recv(&WsMsg::Text("hello".into())));
    assert!(!should_refresh_last_recv(&WsMsg::Close(None)));
}

#[test]
fn lark_user_allowed_exact() {
    let ch = make_channel();
    assert!(ch.is_user_allowed("ou_testuser123"));
    assert!(!ch.is_user_allowed("ou_other"));
}

#[test]
fn lark_user_allowed_wildcard() {
    let ch = LarkChannel::new(
        "id".into(),
        "secret".into(),
        "token".into(),
        None,
        vec!["*".into()],
    );
    assert!(ch.is_user_allowed("ou_anyone"));
}

#[test]
fn lark_user_denied_empty() {
    let ch = LarkChannel::new("id".into(), "secret".into(), "token".into(), None, vec![]);
    assert!(!ch.is_user_allowed("ou_anyone"));
}

#[test]
fn lark_parse_challenge() {
    let ch = make_channel();
    let payload = serde_json::json!({
        "challenge": "abc123",
        "token": "test_verification_token",
        "type": "url_verification"
    });
    // Challenge payloads should not produce messages
    let msgs = ch.parse_event_payload(&payload);
    assert!(msgs.is_empty());
}

#[test]
fn lark_parse_valid_text_message() {
    let ch = make_channel();
    let payload = serde_json::json!({
        "header": {
            "event_type": "im.message.receive_v1"
        },
        "event": {
            "sender": {
                "sender_id": {
                    "open_id": "ou_testuser123"
                }
            },
            "message": {
                "message_type": "text",
                "content": "{\"text\":\"Hello OpenHuman!\"}",
                "chat_id": "oc_chat123",
                "create_time": "1699999999000"
            }
        }
    });

    let msgs = ch.parse_event_payload(&payload);
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0].content, "Hello OpenHuman!");
    assert_eq!(msgs[0].sender, "oc_chat123");
    assert_eq!(msgs[0].channel, "lark");
    assert_eq!(msgs[0].timestamp, 1_699_999_999);
}

#[test]
fn lark_parse_unauthorized_user() {
    let ch = make_channel();
    let payload = serde_json::json!({
        "header": { "event_type": "im.message.receive_v1" },
        "event": {
            "sender": { "sender_id": { "open_id": "ou_unauthorized" } },
            "message": {
                "message_type": "text",
                "content": "{\"text\":\"spam\"}",
                "chat_id": "oc_chat",
                "create_time": "1000"
            }
        }
    });

    let msgs = ch.parse_event_payload(&payload);
    assert!(msgs.is_empty());
}

#[test]
fn lark_parse_non_text_message_skipped() {
    let ch = LarkChannel::new(
        "id".into(),
        "secret".into(),
        "token".into(),
        None,
        vec!["*".into()],
    );
    let payload = serde_json::json!({
        "header": { "event_type": "im.message.receive_v1" },
        "event": {
            "sender": { "sender_id": { "open_id": "ou_user" } },
            "message": {
                "message_type": "image",
                "content": "{}",
                "chat_id": "oc_chat"
            }
        }
    });

    let msgs = ch.parse_event_payload(&payload);
    assert!(msgs.is_empty());
}

#[test]
fn lark_parse_empty_text_skipped() {
    let ch = LarkChannel::new(
        "id".into(),
        "secret".into(),
        "token".into(),
        None,
        vec!["*".into()],
    );
    let payload = serde_json::json!({
        "header": { "event_type": "im.message.receive_v1" },
        "event": {
            "sender": { "sender_id": { "open_id": "ou_user" } },
            "message": {
                "message_type": "text",
                "content": "{\"text\":\"\"}",
                "chat_id": "oc_chat"
            }
        }
    });

    let msgs = ch.parse_event_payload(&payload);
    assert!(msgs.is_empty());
}

#[test]
fn lark_parse_wrong_event_type() {
    let ch = make_channel();
    let payload = serde_json::json!({
        "header": { "event_type": "im.chat.disbanded_v1" },
        "event": {}
    });

    let msgs = ch.parse_event_payload(&payload);
    assert!(msgs.is_empty());
}

#[test]
fn lark_parse_missing_sender() {
    let ch = LarkChannel::new(
        "id".into(),
        "secret".into(),
        "token".into(),
        None,
        vec!["*".into()],
    );
    let payload = serde_json::json!({
        "header": { "event_type": "im.message.receive_v1" },
        "event": {
            "message": {
                "message_type": "text",
                "content": "{\"text\":\"hello\"}",
                "chat_id": "oc_chat"
            }
        }
    });

    let msgs = ch.parse_event_payload(&payload);
    assert!(msgs.is_empty());
}

#[test]
fn lark_parse_unicode_message() {
    let ch = LarkChannel::new(
        "id".into(),
        "secret".into(),
        "token".into(),
        None,
        vec!["*".into()],
    );
    let payload = serde_json::json!({
        "header": { "event_type": "im.message.receive_v1" },
        "event": {
            "sender": { "sender_id": { "open_id": "ou_user" } },
            "message": {
                "message_type": "text",
                "content": "{\"text\":\"Hello world 🌍\"}",
                "chat_id": "oc_chat",
                "create_time": "1000"
            }
        }
    });

    let msgs = ch.parse_event_payload(&payload);
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0].content, "Hello world 🌍");
}

#[test]
fn lark_parse_missing_event() {
    let ch = make_channel();
    let payload = serde_json::json!({
        "header": { "event_type": "im.message.receive_v1" }
    });

    let msgs = ch.parse_event_payload(&payload);
    assert!(msgs.is_empty());
}

#[test]
fn lark_parse_invalid_content_json() {
    let ch = LarkChannel::new(
        "id".into(),
        "secret".into(),
        "token".into(),
        None,
        vec!["*".into()],
    );
    let payload = serde_json::json!({
        "header": { "event_type": "im.message.receive_v1" },
        "event": {
            "sender": { "sender_id": { "open_id": "ou_user" } },
            "message": {
                "message_type": "text",
                "content": "not valid json",
                "chat_id": "oc_chat"
            }
        }
    });

    let msgs = ch.parse_event_payload(&payload);
    assert!(msgs.is_empty());
}

#[test]
fn lark_config_serde() {
    use crate::openhuman::config::schema::{LarkConfig, LarkReceiveMode};
    let lc = LarkConfig {
        app_id: "cli_app123".into(),
        app_secret: "secret456".into(),
        encrypt_key: None,
        verification_token: Some("vtoken789".into()),
        allowed_users: vec!["ou_user1".into(), "ou_user2".into()],
        use_feishu: false,
        receive_mode: LarkReceiveMode::default(),
        port: None,
        notify_target: None,
    };
    let json = serde_json::to_string(&lc).unwrap();
    let parsed: LarkConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.app_id, "cli_app123");
    assert_eq!(parsed.app_secret, "secret456");
    assert_eq!(parsed.verification_token.as_deref(), Some("vtoken789"));
    assert_eq!(parsed.allowed_users.len(), 2);
}

#[test]
fn lark_config_toml_roundtrip() {
    use crate::openhuman::config::schema::{LarkConfig, LarkReceiveMode};
    let lc = LarkConfig {
        app_id: "app".into(),
        app_secret: "secret".into(),
        encrypt_key: None,
        verification_token: Some("tok".into()),
        allowed_users: vec!["*".into()],
        use_feishu: false,
        receive_mode: LarkReceiveMode::Webhook,
        port: Some(9898),
        notify_target: None,
    };
    let toml_str = toml::to_string(&lc).unwrap();
    let parsed: LarkConfig = toml::from_str(&toml_str).unwrap();
    assert_eq!(parsed.app_id, "app");
    assert_eq!(parsed.verification_token.as_deref(), Some("tok"));
    assert_eq!(parsed.allowed_users, vec!["*"]);
}

#[test]
fn lark_config_defaults_optional_fields() {
    use crate::openhuman::config::schema::{LarkConfig, LarkReceiveMode};
    let json = r#"{"app_id":"a","app_secret":"s"}"#;
    let parsed: LarkConfig = serde_json::from_str(json).unwrap();
    assert!(parsed.verification_token.is_none());
    assert!(parsed.allowed_users.is_empty());
    assert_eq!(parsed.receive_mode, LarkReceiveMode::Websocket);
    assert!(parsed.port.is_none());
}

#[test]
fn lark_from_config_preserves_mode_and_region() {
    use crate::openhuman::config::schema::{LarkConfig, LarkReceiveMode};

    let cfg = LarkConfig {
        app_id: "cli_app123".into(),
        app_secret: "secret456".into(),
        encrypt_key: None,
        verification_token: Some("vtoken789".into()),
        allowed_users: vec!["*".into()],
        use_feishu: false,
        receive_mode: LarkReceiveMode::Webhook,
        port: Some(9898),
        notify_target: None,
    };

    let ch = LarkChannel::from_config(&cfg);

    assert_eq!(ch.api_base(), LARK_BASE_URL);
    assert_eq!(ch.ws_base(), LARK_WS_BASE_URL);
    assert_eq!(ch.receive_mode, LarkReceiveMode::Webhook);
    assert_eq!(ch.port, Some(9898));
}

#[test]
fn lark_parse_fallback_sender_to_open_id() {
    // When chat_id is missing, sender should fall back to open_id
    let ch = LarkChannel::new(
        "id".into(),
        "secret".into(),
        "token".into(),
        None,
        vec!["*".into()],
    );
    let payload = serde_json::json!({
        "header": { "event_type": "im.message.receive_v1" },
        "event": {
            "sender": { "sender_id": { "open_id": "ou_user" } },
            "message": {
                "message_type": "text",
                "content": "{\"text\":\"hello\"}",
                "create_time": "1000"
            }
        }
    });

    let msgs = ch.parse_event_payload(&payload);
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0].sender, "ou_user");
}

// ── parse_post_content ─────────────────────────────────────────

#[test]
fn parse_post_content_returns_zh_cn_locale_content() {
    let post = serde_json::json!({
        "zh_cn": {
            "title": "标题",
            "content": [[{"tag": "text", "text": "你好"}]]
        }
    })
    .to_string();
    let out = parse_post_content(&post).expect("parsed");
    assert!(out.contains("标题"));
    assert!(out.contains("你好"));
}

#[test]
fn parse_post_content_falls_back_to_en_us_when_zh_cn_missing() {
    let post = serde_json::json!({
        "en_us": {
            "title": "Hello",
            "content": [[{"tag": "text", "text": "world"}]]
        }
    })
    .to_string();
    let out = parse_post_content(&post).expect("parsed");
    assert!(out.contains("Hello"));
    assert!(out.contains("world"));
}

#[test]
fn parse_post_content_returns_none_for_invalid_json() {
    assert!(parse_post_content("not json").is_none());
}

#[test]
fn parse_post_content_handles_links_and_mentions() {
    let post = serde_json::json!({
        "zh_cn": {
            "title": "T",
            "content": [[
                {"tag": "text", "text": "pre "},
                {"tag": "a", "text": "link", "href": "https://x"},
                {"tag": "at", "user_name": "alice"}
            ]]
        }
    })
    .to_string();
    let out = parse_post_content(&post).expect("parsed");
    assert!(out.contains("link"));
    assert!(out.contains("@alice"));
}

#[test]
fn parse_post_content_falls_back_to_href_when_anchor_text_missing() {
    // Anchor without `text` must surface the `href` — otherwise the
    // link is invisible in the rendered message.
    let post = serde_json::json!({
        "zh_cn": {
            "title": "T",
            "content": [[
                {"tag": "text", "text": "see "},
                {"tag": "a", "href": "https://example.com/no-text"}
            ]]
        }
    })
    .to_string();
    let out = parse_post_content(&post).expect("parsed");
    assert!(
        out.contains("https://example.com/no-text"),
        "href fallback should surface when anchor has no text, got: {out}"
    );
}

#[test]
fn parse_post_content_returns_none_when_all_sections_empty() {
    let post = serde_json::json!({ "zh_cn": { "title": "" } }).to_string();
    assert!(parse_post_content(&post).is_none());
}

// ── strip_at_placeholders ──────────────────────────────────────

#[test]
fn strip_at_placeholders_removes_user_tokens() {
    assert_eq!(strip_at_placeholders("hello @_user_1 world"), "hello world");
    assert_eq!(
        strip_at_placeholders("@_user_42 message here"),
        "message here"
    );
}

#[test]
fn strip_at_placeholders_preserves_real_at_mentions() {
    assert_eq!(strip_at_placeholders("hello @alice"), "hello @alice");
}

#[test]
fn strip_at_placeholders_handles_multiple_placeholders() {
    assert_eq!(strip_at_placeholders("@_user_1 hi @_user_2 bye"), "hi bye");
}

// ── should_respond_in_group ────────────────────────────────────

#[test]
fn should_respond_in_group_requires_nonempty_mentions() {
    assert!(!should_respond_in_group(&[]));
    assert!(should_respond_in_group(&[
        serde_json::json!({"key": "val"})
    ]));
}

#[test]
fn should_refresh_last_recv_true_for_binary_ping_pong() {
    use tokio_tungstenite::tungstenite::Message as WsMsg;
    assert!(should_refresh_last_recv(&WsMsg::Binary(vec![1, 2, 3])));
    assert!(should_refresh_last_recv(&WsMsg::Ping(vec![])));
    assert!(should_refresh_last_recv(&WsMsg::Pong(vec![])));
}

#[test]
fn should_refresh_last_recv_false_for_text_and_close() {
    use tokio_tungstenite::tungstenite::Message as WsMsg;
    assert!(!should_refresh_last_recv(&WsMsg::Text("hello".into())));
    assert!(!should_refresh_last_recv(&WsMsg::Close(None)));
}

#[test]
fn lark_new_stores_fields_and_allowlist() {
    let ch = LarkChannel::new(
        "app_id".into(),
        "secret".into(),
        "verify".into(),
        Some(3001),
        vec!["u1".into(), "u2".into()],
    );
    assert_eq!(ch.app_id, "app_id");
    assert_eq!(ch.port, Some(3001));
    assert_eq!(ch.allowed_users.len(), 2);
}

#[test]
fn lark_is_user_allowed_wildcard_allows_everyone() {
    let ch = LarkChannel::new("a".into(), "s".into(), "v".into(), None, vec!["*".into()]);
    assert!(ch.is_user_allowed("anyone"));
}

#[test]
fn lark_is_user_allowed_empty_allowlist_blocks_everyone() {
    // Empty allowlist matches nothing — explicit guard against the
    // "accidentally allowing all users" bug.
    let ch = LarkChannel::new("a".into(), "s".into(), "v".into(), None, vec![]);
    assert!(!ch.is_user_allowed("anyone"));
}

#[test]
fn lark_is_user_allowed_respects_allowlist() {
    let ch = LarkChannel::new("a".into(), "s".into(), "v".into(), None, vec!["u1".into()]);
    assert!(ch.is_user_allowed("u1"));
    assert!(!ch.is_user_allowed("u2"));
}

#[test]
fn lark_parse_event_payload_empty_object_returns_no_messages() {
    let ch = make_channel();
    let msgs = ch.parse_event_payload(&serde_json::json!({}));
    assert!(msgs.is_empty());
}

#[test]
fn lark_parse_event_payload_ignores_unsupported_message_type() {
    let ch = make_channel();
    let payload = serde_json::json!({
        "header": { "event_type": "im.message.receive_v1" },
        "event": {
            "sender": { "sender_id": { "open_id": "ou_testuser123" } },
            "message": {
                "message_type": "image",
                "content": r#"{"image_key":"abc"}"#,
                "create_time": "1700000000000",
                "chat_id": "chat_xyz"
            }
        }
    });
    let msgs = ch.parse_event_payload(&payload);
    assert!(msgs.is_empty());
}

#[test]
fn lark_parse_event_payload_empty_sender_returns_no_messages() {
    let ch = make_channel();
    let payload = serde_json::json!({
        "header": { "event_type": "im.message.receive_v1" },
        "event": {
            "sender": { "sender_id": { "open_id": "" } },
            "message": {
                "message_type": "text",
                "content": r#"{"text":"hi"}"#,
                "create_time": "1700000000000"
            }
        }
    });
    let msgs = ch.parse_event_payload(&payload);
    assert!(msgs.is_empty());
}

#[test]
fn lark_parse_event_payload_missing_event_returns_empty() {
    let ch = make_channel();
    let payload = serde_json::json!({
        "header": { "event_type": "im.message.receive_v1" }
    });
    let msgs = ch.parse_event_payload(&payload);
    assert!(msgs.is_empty());
}

#[test]
fn lark_parse_event_payload_post_type_extracts_readable_text() {
    let ch = make_channel();
    let post_content = serde_json::json!({
        "zh_cn": {
            "title": "Title",
            "content": [[{"tag":"text","text":"Body"}]]
        }
    })
    .to_string();
    let payload = serde_json::json!({
        "header": { "event_type": "im.message.receive_v1" },
        "event": {
            "sender": { "sender_id": { "open_id": "ou_testuser123" } },
            "message": {
                "message_type": "post",
                "content": post_content,
                "create_time": "1700000000000",
                "chat_id": "chat_xyz"
            }
        }
    });
    let msgs = ch.parse_event_payload(&payload);
    assert_eq!(msgs.len(), 1);
    assert!(msgs[0].content.contains("Title"));
}

// ── markdown → lark_md normalization ───────────────────────────

#[test]
fn normalize_passes_inline_markup_through_unchanged() {
    // lark_md renders these natively — must not be mangled.
    let md = "**bold** and *italic* and ~~strike~~ and [link](https://x) and `code`";
    assert_eq!(normalize_markdown_for_lark(md), md);
}

#[test]
fn normalize_converts_atx_headings_to_bold() {
    assert_eq!(normalize_markdown_for_lark("# Title"), "**Title**");
    assert_eq!(
        normalize_markdown_for_lark("### Sub heading"),
        "**Sub heading**"
    );
    assert_eq!(normalize_markdown_for_lark("###### Deep"), "**Deep**");
}

#[test]
fn normalize_leaves_seven_hashes_alone() {
    // 7+ hashes is not a valid ATX heading — pass through untouched.
    let md = "####### not a heading";
    assert_eq!(normalize_markdown_for_lark(md), md);
}

#[test]
fn normalize_requires_space_after_hashes() {
    // `#tag` is not a heading.
    assert_eq!(normalize_markdown_for_lark("#tag"), "#tag");
}

#[test]
fn normalize_strips_fence_lines_keeps_code_body() {
    let md = "before\n```rust\nlet x = 1;\n```\nafter";
    assert_eq!(normalize_markdown_for_lark(md), "before\nlet x = 1;\nafter");
}

#[test]
fn normalize_handles_tilde_fences() {
    let md = "~~~\ncode\n~~~";
    assert_eq!(normalize_markdown_for_lark(md), "code");
}

#[test]
fn normalize_drops_table_separator_row_keeps_rows() {
    let md = "| A | B |\n|---|---|\n| 1 | 2 |";
    assert_eq!(normalize_markdown_for_lark(md), "| A | B |\n| 1 | 2 |");
}

#[test]
fn normalize_drops_aligned_table_separator() {
    let md = "| A | B |\n| :--- | ---: |\n| 1 | 2 |";
    assert_eq!(normalize_markdown_for_lark(md), "| A | B |\n| 1 | 2 |");
}

#[test]
fn normalize_keeps_data_rows_that_merely_contain_dashes() {
    // A real content row with a dash must NOT be mistaken for a separator.
    let md = "| a-b | c |";
    assert_eq!(normalize_markdown_for_lark(md), "| a-b | c |");
}

#[test]
fn markdown_to_lark_card_builds_interactive_payload() {
    let card = markdown_to_lark_card("# Hi\nbody");
    assert_eq!(card["config"]["wide_screen_mode"], serde_json::json!(true));
    let elements = card["elements"].as_array().expect("elements array");
    assert_eq!(elements.len(), 1);
    assert_eq!(elements[0]["tag"], "markdown");
    assert_eq!(elements[0]["content"], "**Hi**\nbody");
}

// ── markdown table → Feishu schema-2.0 table element ───────────

#[test]
fn split_segments_no_table_is_single_text() {
    let segs = split_markdown_segments("just some **prose**\nmore text");
    assert_eq!(segs.len(), 1);
    assert!(matches!(&segs[0], MdSegment::Text(_)));
}

#[test]
fn split_segments_detects_table_between_prose() {
    let md = "before\n| A | B |\n|---|---|\n| 1 | 2 |\n| 3 | 4 |\nafter";
    let segs = split_markdown_segments(md);
    assert_eq!(segs.len(), 3, "text, table, text");
    match &segs[1] {
        MdSegment::Table(t) => {
            assert_eq!(t.headers, vec!["A", "B"]);
            assert_eq!(t.rows.len(), 2);
            assert_eq!(t.rows[0], vec!["1", "2"]);
            assert_eq!(t.rows[1], vec!["3", "4"]);
        }
        _ => panic!("segment 1 should be a table"),
    }
    assert!(matches!(&segs[0], MdSegment::Text(t) if t == "before"));
    assert!(matches!(&segs[2], MdSegment::Text(t) if t == "after"));
}

#[test]
fn split_table_row_strips_inline_markdown_in_cells() {
    let cells = split_table_row("| **8.0d** | `x` | ~~old~~ |");
    assert_eq!(cells, vec!["8.0d", "x", "old"]);
}

#[test]
fn table_pipes_in_fence_are_not_parsed_as_table() {
    let md = "```\n| A | B |\n|---|---|\n```";
    let segs = split_markdown_segments(md);
    // All prose (inside a code fence), no table.
    assert!(segs.iter().all(|s| matches!(s, MdSegment::Text(_))));
}

#[test]
fn card_with_table_uses_schema_2_and_table_element() {
    let md = "Your quota:\n| Type | Remaining |\n|---|---|\n| AL | **8.0d** |";
    let card = markdown_to_lark_card(md);
    assert_eq!(card["schema"], "2.0");
    let elements = card["body"]["elements"]
        .as_array()
        .expect("body.elements array");
    // First a markdown prose element, then the table.
    assert_eq!(elements[0]["tag"], "markdown");
    let table = &elements[1];
    assert_eq!(table["tag"], "table");
    let cols = table["columns"].as_array().expect("columns");
    assert_eq!(cols.len(), 2);
    assert_eq!(cols[0]["display_name"], "Type");
    assert_eq!(cols[1]["display_name"], "Remaining");
    let rows = table["rows"].as_array().expect("rows");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["c0"], "AL");
    assert_eq!(rows[0]["c1"], "8.0d");
}

#[test]
fn card_without_table_stays_schema_1() {
    let card = markdown_to_lark_card("no table here, just **bold**");
    assert!(card.get("schema").is_none());
    assert_eq!(card["elements"][0]["tag"], "markdown");
}

#[test]
fn table_tolerates_ragged_rows() {
    // A short row must not drop the table; missing cells render empty.
    let md = "| A | B | C |\n|---|---|---|\n| 1 | 2 |";
    let card = markdown_to_lark_card(md);
    let table = &card["body"]["elements"][0];
    assert_eq!(table["columns"].as_array().unwrap().len(), 3);
    let row0 = &table["rows"][0];
    assert_eq!(row0["c0"], "1");
    assert_eq!(row0["c1"], "2");
    assert_eq!(row0["c2"], "");
}

// ── receive_id_type_for ────────────────────────────────────────

#[test]
fn receive_id_type_open_id_for_ou_prefix() {
    assert_eq!(receive_id_type_for("ou_abc123"), "open_id");
}

#[test]
fn receive_id_type_chat_id_for_oc_prefix() {
    assert_eq!(receive_id_type_for("oc_chat456"), "chat_id");
}

#[test]
fn receive_id_type_defaults_to_chat_id() {
    // Anything not an ou_ open_id (incl. inbound reply chat IDs) → chat_id.
    assert_eq!(receive_id_type_for("something_else"), "chat_id");
    assert_eq!(receive_id_type_for(""), "chat_id");
}

#[test]
fn lark_config_defaults_notify_target_none() {
    use crate::openhuman::config::schema::LarkConfig;
    let json = r#"{"app_id":"a","app_secret":"s"}"#;
    let parsed: LarkConfig = serde_json::from_str(json).unwrap();
    assert!(parsed.notify_target.is_none());
}

#[test]
fn parse_member_total_reads_two_member_group() {
    let body = r#"{"code":0,"msg":"success","data":{"member_total":2,"items":[]}}"#;
    assert_eq!(parse_member_total(body).unwrap(), 2);
}

#[test]
fn parse_member_total_reads_larger_group() {
    let body = r#"{"code":0,"data":{"member_total":5}}"#;
    assert_eq!(parse_member_total(body).unwrap(), 5);
}

#[test]
fn parse_member_total_errors_on_business_code() {
    let body = r#"{"code":99991672,"msg":"no permission"}"#;
    let err = parse_member_total(body).unwrap_err().to_string();
    assert!(err.contains("99991672"), "err was: {err}");
}

#[test]
fn parse_member_total_errors_on_missing_total() {
    let body = r#"{"code":0,"data":{}}"#;
    assert!(parse_member_total(body).is_err());
}
