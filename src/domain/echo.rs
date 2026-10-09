//! The others' speech heard again by the user's microphone: a line that
//! repeats, mostly word for word, what they just said.

/// How far back, in seconds, the others' lines are compared.
pub const WINDOW_S: f64 = 20.0;
/// Shorter lines are always kept: a "yes" or an "ok" is easily said by both.
const MIN_WORDS: usize = 4;
/// The share of a line's words that must follow the others' words, in order.
const REPEATED: f32 = 0.6;

/// Lowercase words, without punctuation.
fn words(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(str::to_lowercase)
        .collect()
}

/// The length of the longest common subsequence of two word lists.
fn common(a: &[String], b: &[String]) -> usize {
    let mut previous = vec![0usize; b.len() + 1];
    for word in a {
        let mut current = vec![0usize; b.len() + 1];
        for (j, other) in b.iter().enumerate() {
            current[j + 1] = if word == other {
                previous[j] + 1
            } else {
                current[j].max(previous[j + 1])
            };
        }
        previous = current;
    }
    previous[b.len()]
}

/// Whether `line` mostly repeats `heard`, the others' lines in the order said.
pub fn repeats(line: &str, heard: &[&str]) -> bool {
    let line = words(line);
    if line.len() < MIN_WORDS {
        return false;
    }
    let heard: Vec<String> = heard.iter().flat_map(|text| words(text)).collect();
    common(&line, &heard) as f32 >= REPEATED * line.len() as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A video heard through a headset: the system audio and what the
    /// microphone made of it (a real case).
    #[test]
    fn a_leaked_copy_repeats_the_others() {
        let said = [
            "O Windhawk tá chegando a nossa versão 2.0 e tá ficando realmente incrível.",
            "Se você não conhece o Windhawk, esse aqui é um gerenciador de mods de código aberto pro Windows que traz uma infinidade de recursos e a cada dia que passa, tem cada vez mais mod chegando aqui, criados pela comunidade. E esse s-",
        ];
        let leaked = "O WinkYou é uma plataforma que funciona na sua versão 2.0, a versão realmente incrível. Se você não conhece o WinkYou, esse aqui é um gerenciador de mods de código aberto pro Windows que traz uma extensibilidade de recursos e a cada dia que passa, tem cada vez mais mod chegando aqui que lhe atraem a comunidade.";
        assert!(repeats(leaked, &said));
    }

    #[test]
    fn the_users_own_words_are_kept() {
        let said = ["Você conhece o Windhawk, o gerenciador de mods pro Windows?"];
        assert!(!repeats(
            "Conheço sim, uso faz tempo no trabalho, gosto bastante.",
            &said
        ));
        assert!(!repeats(
            "Conheço o Windhawk sim, uso os mods dele faz tempo.",
            &said
        ));
        // Short replies are never taken for echoes.
        assert!(!repeats("O Windhawk?", &said));
        assert!(!repeats("Gerenciador de mods pro Windows", &[]));
    }
}
