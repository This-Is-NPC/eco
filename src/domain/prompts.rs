//! The requests sent to the model, built so each one starts as the last one did.

use crate::domain::action::Action;
use crate::domain::session::Entry;
use crate::ports::Message;

const SYSTEM_PROMPT: &str = "\
Você ajuda o usuário com uma nota de voz, enquanto ela é gravada ou depois.
Tipo da nota: {kind}. Título: {title}.
A conversa traz a transcrição da nota, aos poucos: cada linha começa com o
nome de quem falou. {speaker} A transcrição é automática e pode conter erros de
reconhecimento. Entre um trecho e outro, o usuário faz pedidos ou perguntas, e
suas respostas anteriores ficam na conversa. Linhas que começam com \"Nota do
usuário:\" são fatos que o usuário escreveu para você usar; ninguém as disse.
{rules}
Contexto do usuário:
{context}
";

/// The rules a config starts with; the user edits them in `rules`.
pub const DEFAULT_RULES: &str = "- Responda no idioma da transcrição.
- Seja curto: o usuário pode estar lendo enquanto a conversa continua.
- Use o contexto do usuário quando for relevante.
- Não repita o que uma resposta anterior já disse; avance a partir dela.
- Nunca atribua ao usuário fatos que não estão na transcrição nem no contexto.";

/// What a reviewer is asked, after the draft, unless the user wrote their own.
pub const DEFAULT_REVIEW: &str = "\
Revise a sua resposta acima antes de ela chegar ao usuário. Corte o que não \
está na transcrição, nas notas nem no contexto do usuário: fatos, números, \
empresas, projetos ou resultados inventados, e resultados de um caso atribuídos \
a outro. Mantenha o idioma da conversa e o formato pedido. Não comente a \
revisão: devolva só a resposta final.";

/// The messages that ask a reviewer to rewrite `draft`: the draft's own request,
/// the draft as the answer, then the review.
pub fn review(mut messages: Vec<Message>, draft: &str, prompt: &str) -> Vec<Message> {
    messages.push(Message {
        role: "assistant",
        content: draft.into(),
        cache: false,
    });
    messages.push(Message {
        role: "user",
        content: prompt.into(),
        cache: false,
    });
    messages
}

/// The languages eco translates into, and the window offers: each code with its
/// English name, for a model to read.
pub const LANGUAGES: [(&str, &str); 17] = [
    ("pt", "Brazilian Portuguese"),
    ("en", "English"),
    ("es", "Spanish"),
    ("fr", "French"),
    ("de", "German"),
    ("it", "Italian"),
    ("nl", "Dutch"),
    ("pl", "Polish"),
    ("ru", "Russian"),
    ("uk", "Ukrainian"),
    ("tr", "Turkish"),
    ("ar", "Arabic"),
    ("hi", "Hindi"),
    ("ja", "Japanese"),
    ("ko", "Korean"),
    ("zh", "Chinese"),
    ("sv", "Swedish"),
];

/// A language's English name; the code when it is not in `LANGUAGES`.
pub fn language_name(code: &str) -> &str {
    LANGUAGES
        .iter()
        .find(|(known, _)| *known == code)
        .map_or(code, |(_, name)| name)
}

/// The messages that translate one line or answer into `language`. Only the text
/// goes: translation needs no transcript, and stays fast and cheap.
pub fn translation(language: &str, text: &str) -> Vec<Message> {
    let system = format!(
        "Translate the user's text into {}. Keep its meaning, tone and Markdown. \
Keep names, numbers, dates, code, product names and technical terms as they \
are. Leave any part already in {0} as it is. Reply with the translation only, \
with no comment.",
        language_name(language)
    );
    vec![
        Message {
            role: "system",
            content: system,
            cache: false,
        },
        Message {
            role: "user",
            content: text.into(),
            cache: false,
        },
    ]
}

pub fn action_request(action: &Action) -> String {
    format!(
        "Pedido: {}\n\nFormato da resposta: {}",
        action.prompt, action.format
    )
}

pub fn question_request(question: &str) -> String {
    format!("Pergunta: {question}\n\nResponda de forma curta e direta.")
}

/// The messages for a request, built so each request starts as the last one did.
///
/// The system message and every earlier turn are rebuilt byte for byte from the
/// timeline, and only new speech and the request go at the end, so a provider
/// with prompt caching reuses everything before them. Messages that end a stable
/// stretch carry `cache` for providers that need a mark.
pub fn conversation(
    kind: &str,
    title: &str,
    rules: &str,
    context: &str,
    user_name: Option<&str>,
    entries: &[Entry],
    request: &str,
) -> Vec<Message> {
    let speaker = match user_name {
        Some(name) => format!("As falas de \"{name}\" são do próprio usuário."),
        None => "A transcrição não identifica as falas do usuário.".into(),
    };
    let context = match context.trim() {
        "" => "(nenhum)",
        trimmed => trimmed,
    };
    let title = match title.trim() {
        "" => "(sem título)",
        trimmed => trimmed,
    };
    let rules = match rules.trim() {
        "" => String::new(),
        trimmed => format!("\nRegras (as instruções de um pedido valem sobre elas):\n{trimmed}\n"),
    };
    let system = SYSTEM_PROMPT
        .replace("{kind}", kind)
        .replace("{title}", title)
        .replace("{speaker}", &speaker)
        .replace("{rules}", &rules)
        .replace("{context}", context);
    let mut messages = vec![Message {
        role: "system",
        content: system,
        cache: true,
    }];
    let mut heard: Vec<String> = Vec::new();
    for entry in entries {
        match entry {
            Entry::Speech(speech) => heard.push(format!("{}: {}", speech.who, speech.text)),
            Entry::Note(note) => heard.push(format!("Nota do usuário: {}", note.text)),
            Entry::Suggestion(answer) => {
                let content = turn(&heard, &answer.request, messages.len());
                messages.push(Message {
                    role: "user",
                    content,
                    cache: false,
                });
                messages.push(Message {
                    role: "assistant",
                    content: answer.text.clone(),
                    cache: false,
                });
                heard.clear();
            }
        }
    }
    if messages.len() > 1 {
        messages.last_mut().expect("not empty").cache = true;
    }
    let content = turn(&heard, request, messages.len());
    messages.push(Message {
        role: "user",
        content,
        cache: false,
    });
    messages
}

fn turn(heard: &[String], request: &str, position: usize) -> String {
    let first = position == 1;
    if heard.is_empty() {
        let silence = if first {
            "Nada foi dito ainda."
        } else {
            "Nada novo foi dito desde o último pedido."
        };
        return format!("{silence}\n\n{request}");
    }
    let title = if first {
        "Transcrição até agora:"
    } else {
        "Transcrição desde o último pedido:"
    };
    format!("{title}\n{}\n\n{request}", heard.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::session::{Note, Speech, Suggestion};

    fn ask() -> Action {
        Action {
            name: "ask".into(),
            prompt: "Sugira uma resposta.".into(),
            format: "Tópicos curtos.".into(),
            model: None,
            ..Action::default()
        }
    }

    fn speech(who: &str, text: &str, at: f64) -> Entry {
        Entry::Speech(Speech {
            who: who.into(),
            text: text.into(),
            at,
        })
    }

    fn answered(text: &str, request: &str) -> Entry {
        Entry::Suggestion(Suggestion {
            id: "s1".into(),
            action: "ask".into(),
            model: "m".into(),
            at: 2.0,
            prompt: "ask".into(),
            request: request.into(),
            text: text.into(),
            draft: String::new(),
            done: true,
        })
    }

    #[test]
    fn each_request_starts_exactly_as_the_previous_one() {
        let first_entries = vec![speech("Recrutador", "Como você faria o cache?", 1.0)];
        let first = conversation(
            "meeting",
            "Entrevista",
            DEFAULT_RULES,
            "Sou dev.",
            Some("Eu"),
            &first_entries,
            &action_request(&ask()),
        );
        let mut second_entries = first_entries.clone();
        second_entries.push(answered("Use Redis.", &action_request(&ask())));
        second_entries.push(speech("Recrutador", "E a invalidação?", 3.0));
        let second = conversation(
            "meeting",
            "Entrevista",
            DEFAULT_RULES,
            "Sou dev.",
            Some("Eu"),
            &second_entries,
            &question_request("Ele citou TTL?"),
        );

        assert_eq!(second[0].content, first[0].content);
        assert!(
            first[0]
                .content
                .contains("valem sobre elas):\n- Responda no idioma")
        );
        assert_eq!(second[1].content, first[1].content);
        assert_eq!(
            second[2],
            Message {
                role: "assistant",
                content: "Use Redis.".into(),
                cache: true
            }
        );
        assert!(
            second[3]
                .content
                .starts_with("Transcrição desde o último pedido:\nRecrutador: E a")
        );
        assert!(
            second[3]
                .content
                .ends_with(&question_request("Ele citou TTL?"))
        );
    }

    #[test]
    fn notes_reach_the_model_among_the_lines_heard() {
        let note = Entry::Note(Note {
            id: "n1".into(),
            text: "Migrei um app WPF para MVVM.".into(),
            at: 2.0,
        });
        let entries = [speech("Recrutador", "Como você migrou?", 1.0), note];
        let messages = conversation("meeting", "", "", "", None, &entries, "Pedido: x");
        assert!(messages[0].content.contains("\"Nota do\nusuário:\""));
        assert_eq!(
            messages[1].content,
            "Transcrição até agora:\nRecrutador: Como você migrou?\nNota do usuário: Migrei um app WPF para MVVM.\n\nPedido: x"
        );
    }

    #[test]
    fn cache_marks_end_the_stable_stretches() {
        let messages = conversation(
            "idea",
            "",
            "",
            "",
            None,
            &[answered("Ok.", "Pedido: x")],
            "Pedido: y",
        );
        assert_eq!(
            messages.iter().map(|m| m.cache).collect::<Vec<_>>(),
            [true, false, true, false]
        );
        assert!(messages[0].content.contains("(nenhum)"));
        assert!(!messages[0].content.contains("Regras"));
        assert_eq!(messages[1].content, "Nada foi dito ainda.\n\nPedido: x");
        assert_eq!(
            messages[3].content,
            "Nada novo foi dito desde o último pedido.\n\nPedido: y"
        );
    }

    #[test]
    fn the_system_message_names_the_session() {
        let named = conversation("meeting", " Acme ", "", "", None, &[], "Pedido: x");
        assert!(
            named[0]
                .content
                .contains("Tipo da nota: meeting. Título: Acme.")
        );
        let untitled = conversation("idea", "", "", "", None, &[], "Pedido: x");
        assert!(
            untitled[0]
                .content
                .contains("Tipo da nota: idea. Título: (sem título).")
        );
    }
}
