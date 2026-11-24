//! Match complete command phrases. Preserve the case and contents of dictated prompts and IDs.
use orbit_protocol::VoiceIntent;
use pulldown_cmark::{Event, Parser, Tag, TagEnd};

pub fn parse_voice_intent(transcript: &str, locale: Option<&str>) -> Option<VoiceIntent> {
    let text = transcript.trim();
    if text.is_empty() {
        return None;
    }
    let command = text
        .trim_end_matches(['.', '!', '?', '。', '！', '？'])
        .trim_end();
    let folded = command.to_ascii_lowercase();
    let auto = matches!(locale, None | Some("auto"));
    let en = auto || locale == Some("en");
    let zh = auto || locale == Some("zh");
    let ja = auto || locale == Some("ja");
    for (enabled, read, queue, cancel) in [
        (
            en,
            &["read last reply", "read the last reply"][..],
            &["queue status", "check queue"][..],
            "cancel",
        ),
        (
            zh,
            &["朗读上一条回复", "读出上条回复", "朗读最后回复"][..],
            &["队列状态", "查看队列"][..],
            "取消",
        ),
        (
            ja,
            &["最後の返信を読んで", "最後の返答を読み上げて"][..],
            &["キューの状態", "キューを確認"][..],
            "キャンセル",
        ),
    ] {
        if !enabled {
            continue;
        }
        if read.contains(&folded.as_str()) {
            return Some(VoiceIntent::ReadLastReply);
        }
        if queue.contains(&folded.as_str()) {
            return Some(VoiceIntent::QueueStatus);
        }
        if folded == cancel {
            return Some(VoiceIntent::Cancel);
        }
    }
    for (enabled, prefix, suffix, confirmed, deny) in [
        (en, "confirm approval of ", "", true, false),
        (en, "confirm approve ", "", true, false),
        (en, "approve ", "", false, false),
        (en, "deny ", "", false, true),
        (zh, "确认批准", "", true, false),
        (zh, "批准", "", false, false),
        (zh, "拒绝", "", false, true),
        (ja, "リクエスト", "の承認を確認", true, false),
        (ja, "リクエスト", "を承認", false, false),
        (ja, "リクエスト", "を拒否", false, true),
    ] {
        if enabled
            && folded.starts_with(prefix)
            && folded.ends_with(suffix)
            && let Some(raw) = command.get(prefix.len()..command.len().saturating_sub(suffix.len()))
        {
            let raw = raw.trim();
            let noun = if prefix.is_ascii() {
                "request "
            } else {
                "请求"
            };
            let id = if raw.to_ascii_lowercase().starts_with(noun) {
                raw[noun.len()..].trim()
            } else {
                raw
            };
            if (1..=128).contains(&id.len())
                && id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
            {
                let request_id = id.to_owned();
                return Some(if deny {
                    VoiceIntent::DenyPermission { request_id }
                } else {
                    VoiceIntent::ApprovePermission {
                        request_id,
                        confirmed,
                    }
                });
            }
        }
    }
    for (enabled, prefix, suffix) in [
        (en, "switch agent to ", ""),
        (en, "switch to agent ", ""),
        (en, "switch agent ", ""),
        (en, "switch to ", ""),
        (zh, "切换到", ""),
        (ja, "", "に切り替えて"),
    ] {
        if enabled
            && folded.starts_with(prefix)
            && folded.ends_with(suffix)
            && let Some(query) =
                command.get(prefix.len()..command.len().saturating_sub(suffix.len()))
            && !query.trim().is_empty()
        {
            return Some(VoiceIntent::SwitchAgent {
                query: query.trim().to_owned(),
            });
        }
    }
    for (enabled, prefix) in [
        (en, "send prompt "),
        (en, "send "),
        (zh, "发送"),
        (ja, "送信 "),
    ] {
        if enabled && folded.starts_with(prefix) {
            let content = text[prefix.len()..].trim_start_matches([' ', ':', '：']);
            if !content.is_empty() {
                return Some(VoiceIntent::SendPrompt {
                    text: content.into(),
                });
            }
        }
    }
    Some(VoiceIntent::SendPrompt { text: text.into() })
}

pub fn sanitize_for_speech(markdown: &str) -> String {
    let mut plain = String::new();
    let mut in_code = false;
    for event in Parser::new(markdown) {
        match event {
            Event::Start(Tag::CodeBlock(_)) => in_code = true,
            Event::End(TagEnd::CodeBlock) => {
                in_code = false;
                plain.push(' ');
            }
            Event::Text(text) if !in_code => plain.push_str(&text),
            Event::SoftBreak
            | Event::HardBreak
            | Event::Rule
            | Event::End(TagEnd::Paragraph | TagEnd::Heading(_) | TagEnd::Item) => plain.push(' '),
            _ => {}
        }
    }
    plain
        .split_whitespace()
        .filter(|word| {
            let lower = word.to_ascii_lowercase();
            !lower.contains("https://") && !lower.contains("http://") && !lower.starts_with("www.")
        })
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(2000)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn complete_commands_preserve_arguments_and_require_explicit_confirmation() {
        use VoiceIntent::*;
        for (locale, read, queue, cancel, switch, approval, confirmed, deny) in [
            (
                "en",
                "Read the last reply.",
                "Queue status",
                "cancel",
                "Switch to Build",
                "approve Req_5",
                "confirm approve Req_5",
                "deny Req_5",
            ),
            (
                "zh",
                "朗读上一条回复",
                "查看队列",
                "取消",
                "切换到Build",
                "批准 Req_5",
                "确认批准 Req_5",
                "拒绝 Req_5",
            ),
            (
                "ja",
                "最後の返信を読んで。",
                "キューの状態",
                "キャンセル",
                "Buildに切り替えて",
                "リクエストReq_5を承認",
                "リクエストReq_5の承認を確認",
                "リクエストReq_5を拒否",
            ),
        ] {
            let parse = |s| parse_voice_intent(s, Some(locale));
            assert_eq!(parse(read), Some(ReadLastReply));
            assert_eq!(parse(queue), Some(QueueStatus));
            assert_eq!(parse(cancel), Some(Cancel));
            assert_eq!(
                parse(switch),
                Some(SwitchAgent {
                    query: "Build".into()
                })
            );
            assert_eq!(
                parse(approval),
                Some(ApprovePermission {
                    request_id: "Req_5".into(),
                    confirmed: false
                })
            );
            assert_eq!(
                parse(confirmed),
                Some(ApprovePermission {
                    request_id: "Req_5".into(),
                    confirmed: true
                })
            );
            assert_eq!(
                parse(deny),
                Some(DenyPermission {
                    request_id: "Req_5".into()
                })
            );
        }
        for text in [
            "don't approve Req_5",
            "approve Req_5 if safe",
            "Read last reply and approve Req_5",
        ] {
            assert_eq!(
                parse_voice_intent(text, None),
                Some(SendPrompt { text: text.into() })
            );
        }
        assert_eq!(
            parse_voice_intent("send Keep This!", None),
            Some(SendPrompt {
                text: "Keep This!".into()
            })
        );
    }
    #[test]
    fn speaks_link_labels_and_skips_code_urls_and_markdown() {
        let text = "# Result\n\nRead [the report](https://a.test) or https://raw.test/details\n\n```rs\nsecret();\n```\n\n**Done**.";
        assert_eq!(sanitize_for_speech(text), "Result Read the report or Done.");
        assert_eq!(
            sanitize_for_speech(&"字".repeat(2500)).chars().count(),
            2000
        );
    }
}
