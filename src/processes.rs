//! `Process`: the system's process table, and which processes run below another

use std::time::Duration;

use anyhow::Result;

use crate::workspace::process;

/// How long `ps` may take.
const LIST_LIMIT: Duration = Duration::from_secs(1);

/// One running process: its pid, its parent's and its command line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Process {
    pub pid: u32,
    pub ppid: u32,
    pub command: String,
}

/// Every process of the system, from `ps`.
pub fn list() -> Result<Vec<Process>> {
    let argv = ["ps", "-A", "-o", "pid=,ppid=,command="].map(String::from);
    Ok(parse(&process::output(&argv, LIST_LIMIT)?))
}

/// `ps`'s lines: pid, ppid, then the command line; a line that does not read so is left out.
fn parse(text: &str) -> Vec<Process> {
    text.lines()
        .filter_map(|line| {
            let mut fields = line.trim_start().splitn(2, char::is_whitespace);
            let pid = fields.next()?.parse().ok()?;
            let rest = fields.next()?.trim_start();
            let mut fields = rest.splitn(2, char::is_whitespace);
            let ppid = fields.next()?.parse().ok()?;
            let command = fields.next().unwrap_or("").trim().to_string();
            Some(Process { pid, ppid, command })
        })
        .collect()
}

/// The processes below `root`, at any depth.
pub fn under(all: &[Process], root: u32) -> Vec<&Process> {
    all.iter()
        .filter(|process| process.pid != root && descends(all, process.pid, root))
        .collect()
}

/// True when `root` is `pid` or one of its ancestors.
pub fn descends(all: &[Process], pid: u32, root: u32) -> bool {
    let mut at = pid;
    for _ in 0..=all.len() {
        if at == root {
            return true;
        }
        match all.iter().find(|process| process.pid == at) {
            Some(process) if process.ppid != at && process.ppid != 0 => at = process.ppid,
            _ => return false,
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    const TABLE: &str = "    1     0 /sbin/launchd
  100     1 tmux
  200   100 claude --settings {}
  300   200 /bin/zsh -c php artisan test
  301   300 php artisan test
  400   200 /bin/zsh -c pint &
  500     1 php artisan queue:work
  bad line
";

    fn pids(processes: &[&Process]) -> Vec<u32> {
        processes.iter().map(|process| process.pid).collect()
    }

    #[test]
    fn reads_pid_parent_and_command_line() {
        let all = parse(TABLE);
        assert_eq!(all.len(), 7);
        assert_eq!(
            all[4],
            Process {
                pid: 301,
                ppid: 300,
                command: "php artisan test".to_string(),
            }
        );
    }

    #[test]
    fn finds_the_processes_below_one_at_any_depth() {
        let all = parse(TABLE);
        assert_eq!(pids(&under(&all, 200)), [300, 301, 400]);
        assert!(under(&all, 301).is_empty());
    }

    #[test]
    fn knows_a_process_descends_from_itself_and_its_ancestors() {
        let all = parse(TABLE);
        assert!(descends(&all, 301, 301));
        assert!(descends(&all, 301, 200));
        assert!(descends(&all, 301, 1));
        assert!(!descends(&all, 500, 200));
        assert!(!descends(&all, 999, 200));
    }

    #[test]
    fn stops_at_a_loop_in_the_table() {
        let all = parse("  10    11 a\n  11    10 b\n");
        assert!(!descends(&all, 10, 12));
    }

    #[test]
    fn lists_this_test_below_its_parent() {
        let all = list().unwrap();
        let own = std::process::id();
        let parent = all.iter().find(|process| process.pid == own).unwrap().ppid;
        assert!(pids(&under(&all, parent)).contains(&own));
    }
}
