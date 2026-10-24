//! A situation: deal, tasks with owners, and tricks played so far.

use crate::card::{Card, CardSet};
use crate::catalogue::TaskCard;
use crate::game::Game;
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Owner {
    /// Player index (0-based; `Player1` is 0).
    Player(u8),
    /// Unknown owner: the draft search tries every legal owner.
    Unknown,
    /// Rule cards apply to the whole crew.
    Crew,
}

#[derive(Debug, Clone)]
pub struct Assignment {
    pub card: &'static TaskCard,
    pub owner: Owner,
}

/// A trick as entered: leader and cards in play order. The last one may be incomplete.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayedTrick {
    pub leader: u8,
    pub cards: Vec<Card>,
}

#[derive(Debug, Clone, Default)]
pub struct Situation {
    pub title: Option<String>,
    pub mission: Option<u32>,
    pub target: Option<u32>,
    pub sealed: bool,
    /// Original hands as dealt.
    pub hands: Vec<CardSet>,
    pub tasks: Vec<Assignment>,
    pub played: Vec<PlayedTrick>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplayError {
    /// 0-based index into `played`.
    pub trick: usize,
    pub msg: String,
}

impl fmt::Display for ReplayError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "trick{}: {}", self.trick + 1, self.msg)
    }
}

impl Situation {
    pub fn players(&self) -> usize {
        self.hands.len()
    }

    /// Sum of printed difficulty of the task cards (rule cards count 0).
    pub fn difficulty_sum(&self) -> u32 {
        self.tasks.iter().map(|a| a.card.difficulty_for(self.players()) as u32).sum()
    }

    /// The game after replaying the played tricks, checking leaders and legality.
    pub fn game(&self) -> Result<Game, ReplayError> {
        let mut g = Game::new(&self.hands).map_err(|e| ReplayError { trick: 0, msg: e.to_string() })?;
        for (i, t) in self.played.iter().enumerate() {
            let err = |msg: String| ReplayError { trick: i, msg };
            if t.leader != g.leader {
                return Err(err(format!(
                    "Player{} leads, but Player{} won the previous trick",
                    t.leader + 1,
                    g.leader + 1
                )));
            }
            if i + 1 < self.played.len() && t.cards.len() != g.n {
                return Err(err(format!("only the last trick may be incomplete ({} cards)", t.cards.len())));
            }
            if t.cards.len() > g.n {
                return Err(err(format!("{} cards for {} players", t.cards.len(), g.n)));
            }
            for &c in &t.cards {
                let p = g.to_play();
                if !g.hands[p].contains(c) {
                    return Err(err(format!("Player{} does not hold {c}", p + 1)));
                }
                if !g.legal_moves().contains(c) {
                    return Err(err(format!("Player{} must follow suit and may not play {c}", p + 1)));
                }
                g.play(c);
            }
        }
        Ok(g)
    }
}
