use dart_core::dart_syntax::{SemanticKind, SemanticMod, flutter_overlay_semantic_tokens};
use openidea_plugin::host;
use openidea_plugin::semantic_tokens::{Document, SemanticToken, SemanticTokens, TokenKind, TokenModifier};
use openidea_plugin::{Position, Range};

pub(crate) fn setting_bool(key: &str, default: bool) -> bool {
    host::setting(key)
        .and_then(|raw| serde_json::from_str::<bool>(&raw).ok())
        .unwrap_or(default)
}

pub(crate) fn setting_str(key: &str) -> Option<String> {
    host::setting(key)
        .and_then(|raw| serde_json::from_str::<String>(&raw).ok())
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
}

pub(crate) fn setting_u32(key: &str, default: u32) -> u32 {
    host::setting(key)
        .and_then(|raw| serde_json::from_str::<u64>(&raw).ok())
        .and_then(|n| u32::try_from(n).ok())
        .unwrap_or(default)
}

impl SemanticTokens for crate::FlutterPlugin {
    fn semantic_tokens(&mut self, doc: &Document) -> Vec<SemanticToken> {
        if !setting_bool("semanticHighlighting", true) {
            return Vec::new();
        }
        let raw = if doc.language == "dart" || doc.path.ends_with(".dart") {
            flutter_overlay_semantic_tokens(&doc.text)
        } else {
            return Vec::new();
        };

        raw.into_iter()
            .map(|tok| {
                let mut st = SemanticToken::new(
                    Range::new(
                        Position::new(tok.span.start.line, tok.span.start.character),
                        Position::new(tok.span.end.line, tok.span.end.character),
                    ),
                    map_kind(tok.kind),
                );
                for m in tok.modifiers {
                    st = st.modifier(map_modifier(m));
                }
                st
            })
            .collect()
    }
}

fn map_kind(kind: SemanticKind) -> TokenKind {
    match kind {
        SemanticKind::Namespace => TokenKind::Namespace,
        SemanticKind::Type => TokenKind::Type,
        SemanticKind::Class => TokenKind::Class,
        SemanticKind::Enum => TokenKind::Enum,
        SemanticKind::Interface => TokenKind::Interface,
        SemanticKind::Struct => TokenKind::Struct,
        SemanticKind::TypeParameter => TokenKind::TypeParameter,
        SemanticKind::Parameter => TokenKind::Parameter,
        SemanticKind::Variable => TokenKind::Variable,
        SemanticKind::Property => TokenKind::Property,
        SemanticKind::EnumMember => TokenKind::EnumMember,
        SemanticKind::Event => TokenKind::Event,
        SemanticKind::Function => TokenKind::Function,
        SemanticKind::Method => TokenKind::Method,
        SemanticKind::Macro => TokenKind::Macro,
        SemanticKind::Keyword => TokenKind::Keyword,
        SemanticKind::Modifier => TokenKind::Modifier,
        SemanticKind::Comment => TokenKind::Comment,
        SemanticKind::String => TokenKind::String,
        SemanticKind::Number => TokenKind::Number,
        SemanticKind::Regexp => TokenKind::Regexp,
        SemanticKind::Operator => TokenKind::Operator,
        SemanticKind::Decorator => TokenKind::Decorator,
    }
}

fn map_modifier(m: SemanticMod) -> TokenModifier {
    match m {
        SemanticMod::Declaration => TokenModifier::Declaration,
        SemanticMod::Definition => TokenModifier::Definition,
        SemanticMod::Readonly => TokenModifier::Readonly,
        SemanticMod::Static => TokenModifier::Static,
        SemanticMod::Deprecated => TokenModifier::Deprecated,
        SemanticMod::Abstract => TokenModifier::Abstract,
        SemanticMod::Async => TokenModifier::Async,
        SemanticMod::Documentation => TokenModifier::Documentation,
        SemanticMod::DefaultLibrary => TokenModifier::DefaultLibrary,
    }
}
