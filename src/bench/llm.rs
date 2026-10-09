//! Time to first token and total time of candidate models on a PT-BR suggestion.

use std::time::{Duration, Instant};

use anyhow::Result;
use futures::StreamExt;
use serde_json::{Map, Value, json};

use crate::adapters::http::Endpoint;
use crate::adapters::llm_openai::OpenAIChat;
use crate::domain::action::Action;
use crate::domain::prompts::{DEFAULT_RULES, action_request, conversation};
use crate::domain::session::{Entry, Speech};
use crate::ports::{Chunk, LanguageModel};

const BASE_URL: &str = "https://openrouter.ai/api/v1";
const KEY: &str = "OPENROUTER_API_KEY";
const RUNS: usize = 3;

const CONTEXT: &str = "Desenvolvedor backend (Elixir/Phoenix, Python). Projeto atual: Matome,
API de gestão de documentos com Postgres, S3 e processamento por IA.";

fn candidates() -> Vec<(&'static str, Map<String, Value>)> {
    let minimal = || match json!({"reasoning": {"effort": "minimal"}}) {
        Value::Object(extra) => extra,
        _ => unreachable!(),
    };
    vec![
        ("google/gemini-3.8-flash", minimal()),
        ("google/gemini-3.5-flash-lite", minimal()),
        ("openai/gpt-5.4-mini", minimal()),
        ("anthropic/claude-haiku-4.5", Map::new()),
        ("anthropic/claude-sonnet-5.5", Map::new()),
    ]
}

fn transcript() -> Vec<Entry> {
    [
        ("Cliente", "Beleza, vi a demo, ficou bem legal."),
        ("Eu", "Valeu! A parte de upload versionado já está estável."),
        (
            "Cliente",
            "E como vocês garantem que um cliente não acessa arquivo de outra organização?",
        ),
    ]
    .map(|(who, text)| {
        Entry::Speech(Speech {
            who: who.into(),
            text: text.into(),
            at: 0.0,
        })
    })
    .into()
}

/// Time to first token, total time and the answer of one request.
async fn measure(chat: &OpenAIChat, model: &str) -> Result<(Option<Duration>, Duration, String)> {
    let ask = Action {
        name: "ask".into(),
        prompt: "Sugira o que eu posso responder agora à última fala deles.".into(),
        format: "De 1 a 3 tópicos curtos, prontos para falar em voz alta.".into(),
        model: None,
        ..Action::default()
    };
    let messages = conversation(
        "meeting",
        "Entrevista",
        DEFAULT_RULES,
        CONTEXT,
        Some("Eu"),
        &transcript(),
        &action_request(&ask),
    );
    let started = Instant::now();
    let (mut first, mut answer) = (None, String::new());
    let mut chunks = chat.stream(model, messages);
    while let Some(chunk) = chunks.next().await {
        if let Chunk::Text(text) = chunk.map_err(|e| anyhow::anyhow!(e.0))? {
            first.get_or_insert_with(|| started.elapsed());
            answer.push_str(&text);
        }
    }
    Ok((first, started.elapsed(), answer))
}

fn median(mut values: Vec<Duration>) -> Duration {
    values.sort();
    values[values.len() / 2]
}

pub async fn run() -> Result<()> {
    let key = crate::config::env_key(KEY)?;
    println!("{:32} {:>9} {:>10}", "model", "ttft p50", "total p50");
    for (model, extra) in candidates() {
        let endpoint = Endpoint::new(BASE_URL, Some(key.clone()), Duration::from_secs(60))?;
        let chat = OpenAIChat::new(endpoint, extra);
        let mut results = Vec::new();
        for _ in 0..RUNS {
            match measure(&chat, model).await {
                Ok(result) => results.push(result),
                Err(failure) => {
                    println!("{model:32} failed: {failure}");
                    break;
                }
            }
        }
        if results.len() < RUNS {
            continue;
        }
        // A run with no text counts as never answering.
        let ttft = median(
            results
                .iter()
                .map(|r| r.0.unwrap_or(Duration::MAX))
                .collect(),
        );
        let total = median(results.iter().map(|r| r.1).collect());
        let ttft = if ttft == Duration::MAX {
            "-".into()
        } else {
            format!("{}ms", ttft.as_millis())
        };
        println!("{model:32} {ttft:>9} {:>9}ms", total.as_millis());
        let answer = &results[RUNS - 1].2;
        println!("  {}", answer.trim().replace('\n', "\n  "));
    }
    Ok(())
}
