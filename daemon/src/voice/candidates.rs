//! Who a spoken request is for (AC-166): the daemon's candidates, each with its reason, computed
//! with no model. Overseer chooses among them; a target outside them waits the longer window and is
//! named aloud.

use std::collections::BTreeSet;

/// What the daemon knows about one agent for this.
#[derive(Clone, Debug, Default)]
pub struct Agent {
    pub id: String,
    pub title: String,
    pub repository: String,
    pub branch: Option<String>,
    pub active: bool,
    /// It asked the owner something, or finished, in the last ten minutes.
    pub just_asked: bool,
    /// Files it changed.
    pub files: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Candidate {
    pub id: String,
    pub reason: &'static str,
}

pub struct Context<'a> {
    pub agents: &'a [Agent],
    /// The agent the owner has selected or is tracking.
    pub focus: Option<&'a str>,
    /// The targets of the previous request.
    pub previous: &'a [String],
}

fn words(text: &str) -> Vec<String> {
    text.to_lowercase()
        .replace(['’', '\''], "")
        .split(|c: char| c.is_whitespace() || c == ',' || c == ';' || c == '?' || c == '!')
        .map(|w| {
            w.trim_matches(|c: char| {
                !c.is_alphanumeric() && c != '.' && c != '/' && c != '-' && c != '_'
            })
            .trim_end_matches('.')
            .to_string()
        })
        .filter(|w| !w.is_empty())
        .collect()
}

/// Words of a name that identify it (short words like "the" or "app" do not).
fn significant(name: &str) -> Vec<String> {
    const WEAK: &[&str] = &[
        "the", "and", "for", "app", "agent", "new", "fix", "add", "a", "an", "of", "to", "in", "on",
    ];
    name.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() >= 3 && !WEAK.contains(w))
        .map(String::from)
        .collect()
}

fn phrase_in(said: &[String], phrase: &[&str]) -> bool {
    said.windows(phrase.len())
        .any(|w| w.iter().zip(phrase).all(|(a, b)| a == b))
}

/// The candidates for a request, in order of their reasons' strength; each agent once.
pub fn candidates(text: &str, ctx: &Context) -> Vec<Candidate> {
    let said = words(text);
    let set: BTreeSet<&str> = said.iter().map(String::as_str).collect();
    let mut out: Vec<Candidate> = Vec::new();
    let add = |id: &str, reason: &'static str, out: &mut Vec<Candidate>| {
        if !out.iter().any(|c| c.id == id) {
            out.push(Candidate {
                id: id.to_string(),
                reason,
            });
        }
    };
    // Named: by title, repository or branch, in the order they were said.
    let at = |w: &str| said.iter().position(|x| x == w).unwrap_or(usize::MAX);
    let mut named: Vec<(usize, &Agent)> = Vec::new();
    for a in ctx.agents.iter().filter(|a| a.active) {
        let title = significant(&a.title);
        let by_title = !title.is_empty()
            && (title.iter().all(|w| set.contains(w.as_str()))
                || title
                    .iter()
                    .any(|w| w.len() >= 5 && set.contains(w.as_str())));
        let by_repo = significant(&a.repository)
            .iter()
            .any(|w| w.len() >= 5 && set.contains(w.as_str()));
        let by_branch = a
            .branch
            .as_deref()
            .is_some_and(|b| b.len() >= 4 && set.contains(b.to_lowercase().as_str()));
        if by_title
            || by_branch
            || (by_repo
                && !ctx
                    .agents
                    .iter()
                    .filter(|x| x.active && x.repository == a.repository)
                    .count()
                    .gt(&1))
        {
            let first = title
                .iter()
                .map(|w| at(w))
                .chain(a.branch.iter().map(|b| at(&b.to_lowercase())))
                .min()
                .unwrap_or(usize::MAX);
            named.push((first, a));
        }
    }
    named.sort_by_key(|(i, _)| *i);
    for (_, a) in named {
        add(&a.id, "named", &mut out);
    }
    // Everyone.
    let everyone = ["everyone", "everybody", "all"]
        .iter()
        .any(|w| set.contains(w))
        && (set.contains("everyone")
            || set.contains("everybody")
            || phrase_in(&said, &["all", "agents"])
            || phrase_in(&said, &["all", "of", "them"])
            || phrase_in(&said, &["all", "of", "you"]));
    if everyone {
        for a in ctx.agents.iter().filter(|a| a.active) {
            add(&a.id, "everyone", &mut out);
        }
    }
    // The previous request's targets ("tell them also", "both of them").
    if ["them", "they", "both", "those", "same"]
        .iter()
        .any(|w| set.contains(w))
    {
        for id in ctx.previous {
            if ctx.agents.iter().any(|a| &a.id == id && a.active) {
                add(id, "previous", &mut out);
            }
        }
    }
    // An agent that just asked or finished ("yes, do that", "whoever asked").
    if phrase_in(&said, &["do", "that"])
        || phrase_in(&said, &["go", "ahead"])
        || set.contains("asked")
        || phrase_in(&said, &["the", "one", "that"])
        || phrase_in(&said, &["that", "one"])
        || set.contains("finished")
    {
        for a in ctx.agents.iter().filter(|a| a.just_asked) {
            add(&a.id, "just asked", &mut out);
        }
    }
    // Files mentioned: the agents that changed them.
    for w in said.iter().filter(|w| {
        w.contains('.')
            && w.rsplit('.').next().is_some_and(|ext| {
                (1..=5).contains(&ext.len()) && ext.chars().all(|c| c.is_ascii_alphanumeric())
            })
            || w.contains('/')
    }) {
        for a in ctx.agents.iter().filter(|a| a.active) {
            if a.files
                .iter()
                .any(|f| f.to_lowercase() == *w || f.to_lowercase().ends_with(&format!("/{w}")))
            {
                add(&a.id, "changed that file", &mut out);
            }
        }
    }
    // The agent the owner has in front of them ("this one", or nobody else named).
    if let Some(f) = ctx.focus {
        let this = phrase_in(&said, &["this", "one"])
            || phrase_in(&said, &["this", "agent"])
            || set.contains("it")
            || set.contains("here");
        if (this || out.is_empty()) && ctx.agents.iter().any(|a| a.id == f && a.active) {
            add(f, "selected", &mut out);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn agents() -> Vec<Agent> {
        let a =
            |id: &str, title: &str, repo: &str, branch: &str, just_asked: bool, files: &[&str]| {
                Agent {
                    id: id.into(),
                    title: title.into(),
                    repository: repo.into(),
                    branch: Some(branch.into()),
                    active: true,
                    just_asked,
                    files: files.iter().map(|f| f.to_string()).collect(),
                }
            };
        vec![
            a(
                "r-phone",
                "Phone app",
                "/w/overseer",
                "claude/phone-remote",
                false,
                &["phone/src/gateway.ts", "docs/rfcs/phone-remote.md"],
            ),
            a(
                "r-cont",
                "Continuity",
                "/w/overseer",
                "claude/continuity-gate-l",
                true,
                &["daemon/src/continuity.rs"],
            ),
            a(
                "r-swarm",
                "Swarm mode",
                "/w/overseer",
                "codex/swarm-mode",
                false,
                &["daemon/src/swarm.rs"],
            ),
            a(
                "r-auto",
                "Auto routing",
                "/w/overseer",
                "codex/automode-rfc",
                false,
                &["docs/rfcs/auto-mode.md", "daemon/src/continuity.rs"],
            ),
            a(
                "r-site",
                "Landing page",
                "/w/website",
                "site/landing",
                false,
                &["index.html", "styles.css"],
            ),
            a(
                "r-docs",
                "Docs cleanup",
                "/w/website",
                "site/docs",
                false,
                &["docs/intro.md"],
            ),
        ]
    }

    fn ids(text: &str, focus: Option<&str>, previous: &[&str]) -> Vec<(String, &'static str)> {
        let a = agents();
        let prev: Vec<String> = previous.iter().map(|s| s.to_string()).collect();
        candidates(
            text,
            &Context {
                agents: &a,
                focus,
                previous: &prev,
            },
        )
        .into_iter()
        .map(|c| (c.id, c.reason))
        .collect()
    }

    fn only(list: &[(String, &'static str)], id: &str, reason: &str) -> bool {
        list.len() == 1 && list[0].0 == id && list[0].1 == reason
    }

    /// Forty utterances over six agents in two repositories (AC-166).
    #[test]
    fn forty_utterances_over_six_agents() {
        let named: &[(&str, &str)] = &[
            ("Tell Continuity to use the new wire format.", "r-cont"),
            ("the phone app should use the new wire format", "r-phone"),
            ("Swarm mode can stop now", "r-swarm"),
            ("ask auto routing for a report", "r-auto"),
            ("the landing page needs a darker hero", "r-site"),
            ("Docs cleanup should also fix the intro", "r-docs"),
            ("tell continuity to wait for the phone", "r-cont"),
            ("have the swarm hold off", "r-swarm"),
            ("routing should use the gpt model", "r-auto"),
            ("landing: move the button up", "r-site"),
            ("whoever is on claude/phone-remote should rebase", "r-phone"),
            ("codex/swarm-mode needs main merged in", "r-swarm"),
            ("site/docs has a broken link", "r-docs"),
        ];
        for (text, id) in named {
            let got = ids(text, None, &[]);
            assert!(
                got.first().is_some_and(|c| c.0 == *id && c.1 == "named"),
                "{text}: {got:?}"
            );
        }
        // Two named at once.
        let two = ids(
            "Phone and Continuity should both use the new wire format",
            None,
            &[],
        );
        assert_eq!(
            two.iter().map(|c| c.0.as_str()).collect::<Vec<_>>(),
            vec!["r-phone", "r-cont"]
        );
        assert!(two.iter().all(|c| c.1 == "named"));
        let three = ids(
            "swarm, routing and docs cleanup should stop pushing",
            None,
            &[],
        );
        assert_eq!(three.len(), 3, "{three:?}");
        // Everyone.
        for text in [
            "everyone stop pushing to main",
            "Everybody, pull main before you push.",
            "all agents should run the tests",
            "tell all of them to pause",
        ] {
            let got = ids(text, None, &[]);
            assert_eq!(got.len(), 6, "{text}: {got:?}");
            assert!(
                got.iter().all(|c| c.1 == "everyone" || c.1 == "named"),
                "{text}: {got:?}"
            );
        }
        // The previous request's targets.
        for text in [
            "tell them also to update the ledger",
            "both should rebase",
            "same for those two",
            "they should add tests",
        ] {
            let got = ids(text, None, &["r-phone", "r-cont"]);
            assert_eq!(
                got.iter().map(|c| (c.0.as_str(), c.1)).collect::<Vec<_>>(),
                vec![("r-phone", "previous"), ("r-cont", "previous")],
                "{text}"
            );
        }
        // The agent that just asked.
        for text in [
            "yes, do that",
            "go ahead",
            "tell the one that asked to go on",
            "whoever asked can continue",
        ] {
            assert!(
                only(&ids(text, None, &[]), "r-cont", "just asked"),
                "{text}: {:?}",
                ids(text, None, &[])
            );
        }
        // A file mentioned.
        let f = ids("whoever changed gateway.ts should add tests", None, &[]);
        assert!(only(&f, "r-phone", "changed that file"), "{f:?}");
        let g = ids(
            "the change to daemon/src/continuity.rs broke the build",
            None,
            &[],
        );
        assert_eq!(
            g.iter().map(|c| c.0.as_str()).collect::<Vec<_>>(),
            vec!["r-cont", "r-auto"],
            "{g:?}"
        );
        let h = ids("styles.css is too big", None, &[]);
        assert!(only(&h, "r-site", "changed that file"), "{h:?}");
        // The selected agent.
        for text in [
            "this one should also write tests",
            "tell it to stop",
            "add a changelog here",
            "make it faster",
        ] {
            assert!(
                only(&ids(text, Some("r-swarm"), &[]), "r-swarm", "selected"),
                "{text}: {:?}",
                ids(text, Some("r-swarm"), &[])
            );
        }
        let unnamed = ids("please add tests", Some("r-docs"), &[]);
        assert!(
            only(&unnamed, "r-docs", "selected"),
            "nobody named: the selected agent"
        );
        // Nobody: no focus, no name.
        for text in [
            "please add tests",
            "use the new wire format",
            "run the migrations",
            "what time is the demo",
        ] {
            assert!(
                ids(text, None, &[]).is_empty(),
                "{text}: {:?}",
                ids(text, None, &[])
            );
        }
        // A name of an agent that is not active is never a candidate.
        let mut a = agents();
        a[0].active = false;
        let got: Vec<_> = candidates(
            "tell the phone app to stop",
            &Context {
                agents: &a,
                focus: None,
                previous: &[],
            },
        )
        .into_iter()
        .map(|c| c.id)
        .collect();
        assert!(got.is_empty(), "{got:?}");
    }
}
