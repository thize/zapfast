//! Synthetic, deterministic conversation data: no network, no clock, a seeded
//! generator, so every run (and the verifier) sees the same 10,000 messages.

use crate::markup::{self, Parsed};
use crate::timefmt::{self, SECONDS_PER_DAY};
use gpui::SharedString;

pub const SEED: u64 = 0x5A9F_A57C_0DE5_EED5;
pub const MESSAGE_COUNT: usize = 10_000;
/// The unread divider sits before this many trailing messages.
pub const UNREAD_COUNT: usize = 7;
pub const SELF_NAME: &str = "Você";

pub const PARTICIPANTS: [&str; 5] = [
    "Ana Souza",
    "Bruno Lima",
    "Carla Mendes",
    "João Pedro",
    "Fernanda Oliveira",
];

/// A small, fast, seedable generator (SplitMix64).
#[derive(Clone)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed)
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform in `0..n`.
    pub fn below(&mut self, n: usize) -> usize {
        (self.next_u64() % n as u64) as usize
    }

    /// Uniform in `lo..=hi`.
    pub fn between(&mut self, lo: usize, hi: usize) -> usize {
        lo + self.below(hi - lo + 1)
    }

    /// True with probability `percent` / 100.
    pub fn chance(&mut self, percent: usize) -> bool {
        self.below(100) < percent
    }

    pub fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len())]
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Delivery {
    Sent,
    Delivered,
    Read,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Quote {
    pub sender: String,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reaction {
    pub emoji: &'static str,
    pub count: u8,
}

#[derive(Clone, Debug)]
pub struct Message {
    pub outgoing: bool,
    pub sender: String,
    /// Local wall-clock seconds since the epoch.
    pub timestamp: i64,
    /// Kept for the tests and the part 2 scenes.
    #[cfg_attr(not(test), allow(dead_code))]
    pub raw: String,
    /// `raw` with its markup interpreted: what is shown and selected.
    pub parsed: Parsed,
    /// `parsed.plain`, shared so each frame can hand it to the text element.
    pub shared: SharedString,
    pub quote: Option<Quote>,
    pub reactions: Vec<Reaction>,
    pub delivery: Delivery,
}

impl Message {
    pub fn new(outgoing: bool, sender: &str, timestamp: i64, raw: String) -> Self {
        let parsed = markup::parse(&raw);
        let shared = SharedString::from(parsed.plain.clone());
        Self {
            outgoing,
            sender: sender.to_owned(),
            timestamp,
            raw,
            parsed,
            shared,
            quote: None,
            reactions: Vec::new(),
            delivery: Delivery::Sent,
        }
    }

    pub fn text(&self) -> &str {
        &self.parsed.plain
    }
}

/// One row of the virtualized list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Row {
    /// A day separator, by day number.
    Day(i64),
    Unread(usize),
    Message(usize),
}

#[derive(Clone, Default)]
pub struct Conversation {
    pub messages: Vec<Message>,
    pub rows: Vec<Row>,
    /// Row index of each message.
    pub message_rows: Vec<usize>,
}

impl Conversation {
    pub fn from_messages(messages: Vec<Message>, unread: usize) -> Self {
        let mut convo = Self::default();
        let first_unread = messages.len().saturating_sub(unread);
        convo.messages = messages;
        let mut last_day = None;
        for index in 0..convo.messages.len() {
            let day = timefmt::day_of(convo.messages[index].timestamp);
            if last_day != Some(day) {
                convo.rows.push(Row::Day(day));
                last_day = Some(day);
            }
            if unread > 0 && index == first_unread {
                convo.rows.push(Row::Unread(unread));
            }
            convo.message_rows.push(convo.rows.len());
            convo.rows.push(Row::Message(index));
        }
        convo
    }

    /// Appends a message, with a day separator when it starts a new day.
    /// Returns the number of rows added.
    pub fn push(&mut self, message: Message) -> usize {
        let before = self.rows.len();
        let day = timefmt::day_of(message.timestamp);
        let last_day = self.messages.last().map(|m| timefmt::day_of(m.timestamp));
        if last_day != Some(day) {
            self.rows.push(Row::Day(day));
        }
        self.message_rows.push(self.rows.len());
        self.rows.push(Row::Message(self.messages.len()));
        self.messages.push(message);
        self.rows.len() - before
    }
}

// ---------------------------------------------------------------------------
// Text material

const SENTENCES: &[&str] = &[
    "Bom dia, pessoal! Tudo bem com vocês?",
    "Não consegui terminar o relatório ontem, vou entregar hoje à tarde.",
    "Alguém pode confirmar a reunião de amanhã às 14h?",
    "Obrigado pela ajuda, foi muito útil.",
    "Esse açaí está incrível, vocês precisam provar!",
    "Estou chegando em dez minutos, não saiam sem mim.",
    "A previsão do tempo diz que vai chover o fim de semana inteiro.",
    "Já vi o documento, tem alguns ajustes de acentuação para fazer.",
    "Pode ser na quinta-feira? Na sexta eu estou viajando.",
    "Que ótima notícia! Parabéns pela promoção, mais que merecida.",
    "Ainda não recebi a confirmação do pagamento, será que deu algum problema?",
    "Vou ligar para o João e já te aviso o que ele disse.",
    "Lembrei que preciso comprar pão, leite e ovos antes de ir para casa.",
    "O ônibus atrasou de novo, o coração já está na mão.",
    "Compartilhei a pasta com todo mundo, qualquer coisa me avisem.",
    "Nossa, não acredito que perdemos o último capítulo!",
    "Combinado então, nos vemos lá em frente ao cinema às sete e meia.",
    "Preciso de uma ajudinha com a planilha, as fórmulas não fecham.",
    "Hoje o trânsito está um caos, melhor sair mais cedo.",
    "Feliz aniversário! Que seu dia seja cheio de alegria e muita festa.",
    "Segue em anexo a versão revisada da proposta.",
    "Será que a padaria da esquina ainda está aberta?",
    "Eu avisei que ia dar certo, só precisava de um pouco de paciência.",
    "Ontem à noite assisti ao jogo inteiro, que partida emocionante!",
];

const SHORT: &[&str] = &[
    "ok",
    "beleza",
    "👍",
    "kkkk",
    "sim",
    "não",
    "boa!",
    "valeu",
    "combinado",
    "tá bom",
    "pode ser",
    "já vou",
    "calma aí",
    "😂😂",
    "show",
    "opa",
    "ué",
    "não sei",
    "talvez",
    "hmm",
];

const EMOJI: &[&str] = &[
    "😀",
    "😂",
    "🥹",
    "😍",
    "🙏",
    "👍🏽",
    "👍🏻",
    "🤝🏿",
    "👨‍👩‍👧‍👦",
    "👩🏽‍💻",
    "🧑🏾‍🍳",
    "🏳️‍🌈",
    "🇧🇷",
    "🇵🇹",
    "🇯🇵",
    "❤️",
    "🎉",
    "🔥",
    "☕",
    "🤷🏽‍♀️",
];

const ARABIC: &[&str] = &[
    "مرحبا بكم في المحادثة، كيف حالكم اليوم؟",
    "سنلتقي غدا في الساعة الثالثة بعد الظهر.",
    "شكرا جزيلا على المساعدة، أنت رائع!",
];

const HEBREW: &[&str] = &[
    "שלום לכולם, מה שלומכם היום?",
    "נתראה מחר בשעה שלוש אחר הצהריים.",
    "תודה רבה על העזרה, זה היה ממש עוזר.",
];

const CJK: &[&str] = &[
    "今日は会議があります。明日までに資料を送ってください。",
    "你好，今天下午三点开会，请准时参加。",
    "안녕하세요, 내일 만나서 이야기해요.",
    "ありがとうございます、とても助かりました。",
];

const URL_HOSTS: &[&str] = &[
    "https://www.exemplo.com.br",
    "https://drive.exemplo.org",
    "https://noticias.exemplo.net",
];

const URL_WORDS: &[&str] = &[
    "relatorio",
    "anual",
    "2026",
    "versao",
    "final",
    "documento",
    "compartilhado",
    "pasta",
    "arquivos",
    "reuniao",
    "projeto",
    "orcamento",
    "detalhes",
];

const REACTIONS: &[&str] = &["👍", "❤️", "😂", "😮", "🙏", "🎉"];

fn url(rng: &mut Rng) -> String {
    let mut out = String::from(*rng.pick(URL_HOSTS));
    for _ in 0..rng.between(5, 12) {
        out.push('/');
        out.push_str(rng.pick(URL_WORDS));
        if rng.chance(30) {
            out.push('_');
            out.push_str(rng.pick(URL_WORDS));
        }
    }
    out.push_str("?utm_source=conversa&id=");
    out.push_str(&rng.below(1_000_000_000).to_string());
    out
}

fn sentence(rng: &mut Rng) -> String {
    let base = rng.pick(SENTENCES).to_string();
    match rng.below(100) {
        0..=9 => format!("{base} {}", rng.pick(EMOJI)),
        10..=14 => format!("{base} {}{}", rng.pick(EMOJI), rng.pick(EMOJI)),
        15..=19 => emphasize(&base, rng),
        _ => base,
    }
}

/// Wraps one word of the sentence in WhatsApp markers.
fn emphasize(sentence: &str, rng: &mut Rng) -> String {
    let words: Vec<&str> = sentence.split(' ').collect();
    let at = rng.below(words.len());
    let marker = if rng.chance(50) { '*' } else { '_' };
    words
        .iter()
        .enumerate()
        .map(|(i, word)| {
            let core = word.trim_end_matches(['.', ',', '!', '?']);
            if i == at && core.chars().count() > 2 && core.chars().all(char::is_alphabetic) {
                format!("{marker}{core}{marker}{}", &word[core.len()..])
            } else {
                (*word).to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn line(rng: &mut Rng) -> String {
    match rng.below(100) {
        0..=2 => rng.pick(ARABIC).to_string(),
        3..=5 => rng.pick(HEBREW).to_string(),
        6..=9 => rng.pick(CJK).to_string(),
        10..=13 => format!("Olha isso: {}", url(rng)),
        14..=16 => url(rng),
        17..=24 => rng.pick(SHORT).to_string(),
        25..=27 => format!(
            "*{}* {}",
            rng.pick(&["Atenção", "Aviso", "Importante"]),
            sentence(rng)
        ),
        28..=29 => format!("_{}_", rng.pick(SENTENCES)),
        _ => sentence(rng),
    }
}

fn body(rng: &mut Rng) -> String {
    // 1 to 30 lines, mostly short.
    let lines = match rng.below(100) {
        0..=64 => 1,
        65..=84 => rng.between(2, 4),
        85..=96 => rng.between(5, 12),
        _ => rng.between(13, 30),
    };
    if lines > 5 && rng.chance(50) {
        // A list, one item per line.
        return (1..=lines)
            .map(|n| format!("{n}. {}", rng.pick(SENTENCES)))
            .collect::<Vec<_>>()
            .join("\n");
    }
    (0..lines).map(|_| line(rng)).collect::<Vec<_>>().join("\n")
}

fn truncate_chars(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        text.to_owned()
    } else {
        let cut: String = text.chars().take(max).collect();
        format!("{cut}…")
    }
}

/// The 10,000-message conversation the spike scrolls through.
pub fn generate(seed: u64, count: usize) -> Conversation {
    let mut rng = Rng::new(seed);
    let mut now = timefmt::days_from_civil(2026, 9, 1) * SECONDS_PER_DAY + 7 * 3600 + 20 * 60;
    let mut messages: Vec<Message> = Vec::with_capacity(count);
    for index in 0..count {
        // Mostly minutes apart, sometimes a long pause into the next day.
        now += match rng.below(100) {
            0..=2 => rng.between(8 * 3600, 20 * 3600) as i64,
            3..=15 => rng.between(20 * 60, 3 * 3600) as i64,
            _ => rng.between(5, 15 * 60) as i64,
        };
        let outgoing = rng.chance(40);
        let sender = if outgoing {
            SELF_NAME
        } else {
            rng.pick(&PARTICIPANTS)
        };
        let mut message = Message::new(outgoing, sender, now, body(&mut rng));
        if index > 0 && rng.chance(9) {
            let back = rng.between(1, index.min(40));
            let target = &messages[index - back];
            let first_line = target.text().lines().next().unwrap_or("");
            message.quote = Some(Quote {
                sender: target.sender.clone(),
                text: truncate_chars(first_line, 90),
            });
        }
        if rng.chance(11) {
            for _ in 0..rng.between(1, 3) {
                let emoji = *rng.pick(REACTIONS);
                if message.reactions.iter().all(|r| r.emoji != emoji) {
                    message.reactions.push(Reaction {
                        emoji,
                        count: rng.between(1, 5) as u8,
                    });
                }
            }
        }
        message.delivery = if index + 3 >= count {
            Delivery::Sent
        } else if index + 6 >= count {
            Delivery::Delivered
        } else {
            Delivery::Read
        };
        messages.push(message);
    }
    Conversation::from_messages(messages, UNREAD_COUNT)
}

pub fn default_conversation() -> Conversation {
    generate(SEED, MESSAGE_COUNT)
}

// ---------------------------------------------------------------------------
// The chat list on the left

pub struct ChatSummary {
    pub name: &'static str,
    pub preview: &'static str,
    pub time: &'static str,
    pub unread: u32,
    pub muted: bool,
}

pub fn chat_list() -> Vec<ChatSummary> {
    const CHATS: &[(&str, &str, &str, u32)] = &[
        ("Família Souza", "Ana: bom dia, pessoal! 🇧🇷", "08:41", 3),
        ("Bruno Lima", "Combinado, nos vemos lá!", "08:12", 0),
        (
            "Trabalho — Projeto Aurora",
            "Carla: segue em anexo a versão final",
            "07:55",
            12,
        ),
        ("Mamãe ❤️", "Já almoçou, filho?", "Ontem", 1),
        ("Carla Mendes", "Valeu pela ajuda 🙏🏽", "Ontem", 0),
        (
            "Futebol de quinta ⚽",
            "João: quem leva a bola?",
            "Ontem",
            27,
        ),
        ("Fernanda Oliveira", "Foto", "Seg", 0),
        (
            "Condomínio Jardim das Acácias",
            "Assembleia na terça às 19h",
            "Seg",
            5,
        ),
        ("Dr. Ricardo", "Seus exames estão prontos.", "Dom", 0),
        (
            "Clube do Livro 📚",
            "Alguém já leu o último capítulo?",
            "Dom",
            0,
        ),
        ("Pedro Henrique", "kkkkkkk", "Sáb", 0),
        (
            "Viagem Lisboa 2026 ✈️",
            "Marta: reservei o hotel!",
            "Sáb",
            2,
        ),
        ("Padaria do Zé", "Seu pedido saiu para entrega.", "Sex", 0),
        ("Luiza Andrade", "Ligo mais tarde, tá?", "Sex", 0),
        ("Turma da Faculdade", "Reencontro dia 12 🎉", "Qui", 41),
        ("Suporte Técnico", "Chamado #48213 encerrado", "Qui", 0),
        ("Tio Marcos", "Bom dia! Deus abençoe 🙏", "Qua", 0),
        ("Vizinhos do 302", "Alguém ouviu o barulho?", "Qua", 0),
        (
            "Academia Corpo & Mente",
            "Sua aula de amanhã foi confirmada",
            "Ter",
            0,
        ),
        ("Rafaela Costa", "Te mando as fotos amanhã", "Ter", 0),
    ];
    CHATS
        .iter()
        .enumerate()
        .map(|(i, (name, preview, time, unread))| ChatSummary {
            name,
            preview,
            time,
            unread: *unread,
            muted: i % 7 == 5,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generation_is_deterministic() {
        let a = generate(SEED, 600);
        let b = generate(SEED, 600);
        assert_eq!(a.rows, b.rows);
        for (x, y) in a.messages.iter().zip(&b.messages) {
            assert_eq!(x.raw, y.raw);
            assert_eq!(x.timestamp, y.timestamp);
            assert_eq!(x.outgoing, y.outgoing);
            assert_eq!(x.quote, y.quote);
            assert_eq!(x.reactions, y.reactions);
        }
        let other = generate(SEED + 1, 600);
        assert!(
            a.messages
                .iter()
                .zip(&other.messages)
                .any(|(x, y)| x.raw != y.raw)
        );
    }

    #[test]
    fn a_prefix_does_not_depend_on_the_length() {
        // Message n only depends on what came before it.
        let short = generate(SEED, 50);
        let long = generate(SEED, 300);
        for i in 0..40 {
            assert_eq!(short.messages[i].raw, long.messages[i].raw, "{i}");
        }
    }

    #[test]
    fn the_default_conversation_has_ten_thousand_messages() {
        let convo = default_conversation();
        assert_eq!(convo.messages.len(), MESSAGE_COUNT);
        assert_eq!(convo.message_rows.len(), MESSAGE_COUNT);
        let unread = convo
            .rows
            .iter()
            .filter(|r| matches!(r, Row::Unread(_)))
            .count();
        assert_eq!(unread, 1);
        let days = convo
            .rows
            .iter()
            .filter(|r| matches!(r, Row::Day(_)))
            .count();
        assert!(days > 50, "{days}");
        // Timestamps only go forward, so day separators never repeat.
        assert!(
            convo
                .messages
                .windows(2)
                .all(|w| w[0].timestamp < w[1].timestamp)
        );
        for (index, &row) in convo.message_rows.iter().enumerate() {
            assert_eq!(convo.rows[row], Row::Message(index));
        }
    }

    #[test]
    fn the_material_covers_what_the_spike_must_stress() {
        let convo = default_conversation();
        let any = |f: &dyn Fn(&Message) -> bool| convo.messages.iter().any(f);
        assert!(any(&|m| m.outgoing) && any(&|m| !m.outgoing));
        assert!(any(&|m| m.raw.contains("ã") && m.raw.contains("ç")));
        assert!(any(&|m| m.raw.contains('👨')));
        assert!(any(&|m| m.raw.contains("🇧🇷")));
        assert!(any(&|m| m.raw.contains("👍🏽")));
        assert!(any(&|m| m.raw.contains("مرحبا")));
        assert!(any(&|m| m.raw.contains("שלום")));
        assert!(any(&|m| m.raw.contains("会議") || m.raw.contains("你好")));
        assert!(any(&|m| m.raw.contains("https://") && m.raw.len() > 100));
        assert!(any(&|m| m.parsed.runs.iter().any(|r| r.bold)));
        assert!(any(&|m| m.parsed.runs.iter().any(|r| r.italic)));
        assert!(any(&|m| m.quote.is_some()));
        assert!(any(&|m| !m.reactions.is_empty()));
        let line_counts: Vec<usize> = convo
            .messages
            .iter()
            .map(|m| m.raw.lines().count())
            .collect();
        assert_eq!(*line_counts.iter().min().unwrap(), 1);
        assert!(*line_counts.iter().max().unwrap() >= 25);
    }

    #[test]
    fn pushing_adds_a_day_separator_only_on_a_new_day() {
        let mut convo = generate(SEED, 20);
        let last = convo.messages.last().unwrap().timestamp;
        let same_day = Message::new(true, SELF_NAME, last + 1, "oi".into());
        assert_eq!(convo.push(same_day), 1);
        let next_day = Message::new(true, SELF_NAME, last + 2 * SECONDS_PER_DAY, "oi".into());
        assert_eq!(convo.push(next_day), 2);
        assert_eq!(convo.message_rows.len(), convo.messages.len());
        let row = *convo.message_rows.last().unwrap();
        assert_eq!(convo.rows[row], Row::Message(convo.messages.len() - 1));
    }

    #[test]
    fn the_chat_list_has_twenty_entries() {
        assert_eq!(chat_list().len(), 20);
    }
}
