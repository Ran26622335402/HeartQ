//! `/proposals` — list / show / accept / reject auto-generated meta-skill proposals.
//!
//! Proposals are written by dream-enhanced auto_propose under
//! `${HEARTQ_HOME}/proposals/*.json`. Accept promotes to
//! `${HEARTQ_HOME}/meta_skills/<name>/spec.json`.

use heartq_memory::auto_propose::{
    accept_proposal, default_proposals_dir, list_proposals, reject_proposal, show_proposal,
};

use super::super::command::{CommandExecCtx, CommandResult, SlashCommand};

/// `/proposals [list | show <id> | accept <id> | reject <id>]`
pub struct ProposalsCommand;

impl SlashCommand for ProposalsCommand {
    fn name(&self) -> &str {
        "proposals"
    }

    fn description(&self) -> &str {
        "List, show, accept, or reject auto-generated meta-skill proposals"
    }

    fn session_scoped(&self) -> bool {
        false
    }

    fn usage(&self) -> &str {
        "/proposals [list | show <id> | accept <id> | reject <id>]"
    }

    fn takes_args(&self) -> bool {
        true
    }

    fn args_required(&self) -> bool {
        false
    }

    fn arg_placeholder(&self) -> Option<&str> {
        Some("list | show <id> | accept <id> | reject <id>")
    }

    fn run(&self, _ctx: &mut CommandExecCtx, args: &str) -> CommandResult {
        let args = args.trim();
        let mut parts = args.split_whitespace();
        let sub = parts.next().unwrap_or("list");
        match sub {
            "list" | "" => list_all(),
            "show" => match parts.next() {
                Some(id) => show_one(id),
                None => CommandResult::Error("usage: /proposals show <id>".into()),
            },
            "accept" => match parts.next() {
                Some(id) => accept_one(id),
                None => CommandResult::Error("usage: /proposals accept <id>".into()),
            },
            "reject" => match parts.next() {
                Some(id) => reject_one(id),
                None => CommandResult::Error("usage: /proposals reject <id>".into()),
            },
            other => CommandResult::Error(format!(
                "unknown /proposals subcommand: {other:?}\nusage: {}",
                self.usage()
            )),
        }
    }
}

fn list_all() -> CommandResult {
    let dir = default_proposals_dir();
    match list_proposals() {
        Ok(proposals) if proposals.is_empty() => CommandResult::Message(format!(
            "(no proposals in {})",
            dir.display()
        )),
        Ok(proposals) => {
            let mut lines = vec![format!(
                "proposals ({} in {}):",
                proposals.len(),
                dir.display()
            )];
            for p in proposals {
                let steps = p
                    .steps
                    .as_ref()
                    .map(|s| s.len())
                    .unwrap_or(p.steps_sketch.len());
                lines.push(format!(
                    "- [{}] {} ({} steps, source={:?}, status={:?})",
                    p.id, p.title, steps, p.source, p.status
                ));
            }
            CommandResult::Message(lines.join("\n"))
        }
        Err(e) => CommandResult::Error(format!("could not read {}: {e}", dir.display())),
    }
}

fn show_one(id: &str) -> CommandResult {
    match show_proposal(id) {
        Ok(p) => {
            let json = serde_json::to_string_pretty(&p).unwrap_or_else(|_| format!("{p:?}"));
            CommandResult::Message(json)
        }
        Err(e) => CommandResult::Error(format!("proposal show failed: {e}")),
    }
}

fn accept_one(id: &str) -> CommandResult {
    match accept_proposal(id) {
        Ok(spec) => CommandResult::Message(format!(
            "accepted proposal {id} → meta-skill `{}` ({} steps, digest={})",
            spec.name,
            spec.steps.len(),
            spec.digest
        )),
        Err(e) => CommandResult::Error(format!("proposal accept failed: {e}")),
    }
}

fn reject_one(id: &str) -> CommandResult {
    match reject_proposal(id) {
        Ok(()) => CommandResult::Message(format!("rejected (deleted) proposal {id}")),
        Err(e) => CommandResult::Error(format!("proposal reject failed: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use heartq_memory::auto_propose::{propose_from_cooccurrence, save_proposal};
    use heartq_memory::meta_skill::MetaSkillStore;

    #[test]
    fn list_empty_when_dir_absent() {
        let tmp = tempfile::tempdir().unwrap();
        unsafe {
            std::env::set_var("HEARTQ_HOME", tmp.path());
        }
        match list_all() {
            CommandResult::Message(msg) => assert!(msg.contains("no proposals")),
            other => panic!("expected Message, got {other:?}"),
        }
    }

    #[test]
    fn list_shows_saved_proposal() {
        let tmp = tempfile::tempdir().unwrap();
        unsafe {
            std::env::set_var("HEARTQ_HOME", tmp.path());
        }
        let p = propose_from_cooccurrence(&["lint".into(), "test".into()]).unwrap();
        save_proposal(&p).unwrap();
        match list_all() {
            CommandResult::Message(msg) => {
                assert!(msg.contains(&p.title), "msg={msg}");
                assert!(msg.contains(&p.id));
            }
            other => panic!("expected Message, got {other:?}"),
        }
    }

    #[test]
    fn accept_and_reject_via_slash_helpers() {
        let tmp = tempfile::tempdir().unwrap();
        unsafe {
            std::env::set_var("HEARTQ_HOME", tmp.path());
        }
        let p = propose_from_cooccurrence(&["lint".into(), "test".into()]).unwrap();
        let id = p.id.clone();
        save_proposal(&p).unwrap();
        match accept_one(&id) {
            CommandResult::Message(msg) => {
                assert!(msg.contains("accepted"), "msg={msg}");
                assert!(MetaSkillStore::spec_exists(
                    p.name.as_deref().unwrap_or("combine-lint-test")
                ) || msg.contains("meta-skill"));
            }
            other => panic!("expected Message, got {other:?}"),
        }

        let p2 = propose_from_cooccurrence(&["a".into(), "b".into()]).unwrap();
        let id2 = p2.id.clone();
        save_proposal(&p2).unwrap();
        match reject_one(&id2) {
            CommandResult::Message(msg) => assert!(msg.contains("rejected")),
            other => panic!("expected Message, got {other:?}"),
        }
    }
}
