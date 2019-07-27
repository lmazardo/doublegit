use regex::Regex;
use std::collections::HashSet;
use std::ops::Not;
use std::path::Path;
use std::process;

use crate::{Error, Operation, Ref};

fn parse_operation(chr: &str) -> Result<Operation, Error> {
    if chr.len() != 1 {
        return Err(Error::git("Parse error: invalid operation"));
    }
    let chr = chr.as_bytes()[0];
    Ok(match chr {
        b' ' => Operation::FastForward,
        b'+' => Operation::Forced,
        b'-' => Operation::Pruned,
        b't' => Operation::Tag,
        b'*' => Operation::New,
        b'!' => Operation::Reject,
        b'=' => Operation::Noop,
        _ => return Err(Error::git("Parse error: invalid operation")),
    })
}

pub struct FetchOutput {
    pub new: HashSet<Ref>,
    pub changed: HashSet<Ref>,
    pub removed: HashSet<Ref>,
}

pub fn fetch(repository: &Path) -> Result<FetchOutput, Error> {
    let output = process::Command::new("git")
        .args(&["fetch", "--prune", "origin",
                "+refs/tags/*:refs/tags/*",
                "+refs/heads/*:refs/remotes/origin/*"])
        .current_dir(repository)
        .stdin(process::Stdio::null())
        .stdout(process::Stdio::inherit())
        .output()?;
    if !output.status.success() {
        return Err(Error::Git(format!("`git fetch` returned {}",
                                      output.status)));
    }
    parse_fetch_output(&output.stderr)
}

fn parse_fetch_output(output: &[u8]) -> Result<FetchOutput, Error> {
    lazy_static! {
        static ref _RE_FETCH: Regex = Regex::new(
            r"^ ([+t*! -]) +([^ ]+|\[[^\]]+\]) +([^ ]+) +-> +([^ ]+)(?: +(.+))?$"
        ).unwrap();
    }
    let remote = "origin";
    let mut new = HashSet::new();
    let mut changed = HashSet::new();
    let mut removed = HashSet::new();
    for line in output.split(|&b| b == b'\n') {
        let line = std::str::from_utf8(line)
            .map_err(|_| Error::git("Non-utf8 branch name"))?;
        if let Some(m) = _RE_FETCH.captures(line) {
            info!("> {}", line);
            let op = m.get(1).map_or("", |m| m.as_str());
            let summary = m.get(2).map_or("", |m| m.as_str());
            let from = m.get(3).map_or("", |m| m.as_str());
            let to = m.get(4).map_or("", |m| m.as_str());
            let reason = m.get(5).map_or("", |m| m.as_str());

            let op = parse_operation(op)?;
            match op {
                Operation::New => {
                    if !to.contains('/') { // tag
                        new.insert(Ref {
                            remote: remote.into(),
                            name: to.into(),
                            tag: true,
                        });
                    } else {
                        new.insert(Ref::parse_remote_ref(to, remote)?);
                    }
                }
                Operation::FastForward|Operation::Forced => {
                    changed.insert(Ref::parse_remote_ref(to, remote)?);
                }
                Operation::Pruned => {
                    if !to.contains('/') { // tag
                        removed.insert(Ref {
                            remote: remote.into(),
                            name: to.into(),
                            tag: true,
                        });
                    } else {
                        removed.insert(Ref::parse_remote_ref(to, remote)?);
                    }
                }
                Operation::Tag => {
                    changed.insert(Ref {
                        remote: remote.into(),
                        name: to.into(),
                        tag: true,
                    });
                }
                Operation::Reject => {
                    return Err(Error::Git(format!("Error updating ref {}",
                                                  to)));
                }
                Operation::Noop => {}
            }
        } else {
            info!("! {}", line);
        }
    }
    Ok(FetchOutput { new, changed, removed })
}

pub fn get_sha(repository: &Path, refname: &str) -> Result<String, Error> {
    let output = process::Command::new("git")
        .args(&["rev-parse", refname])
        .current_dir(repository)
        .stdin(process::Stdio::null())
        .stderr(process::Stdio::inherit())
        .output()?;
    if !output.status.success() {
        return Err(Error::Git(format!("`git rev-parse` returned {}",
                                      output.status)));
    }
    let sha = std::str::from_utf8(&output.stdout)
        .map_err(|_| Error::git("Non-utf8 sha?!"))?;
    Ok(sha.trim().into())
}

pub fn make_branch(
    repository: &Path,
    name: &str,
    sha: &str,
) -> Result<(), Error> {
    let status = process::Command::new("git")
        .args(&["branch", "-f", name, sha])
        .current_dir(repository)
        .stdin(process::Stdio::null())
        .status()?;
    if !status.success() {
        return Err(Error::Git(format!("`git branch -f` returned {}", status)));
    }
    Ok(())
}

pub fn included_branches(
    repository: &Path, target: &str,
) -> Result<Vec<String>, Error> {
    let output = process::Command::new("git")
        .args(&["branch", "--merged", target])
        .current_dir(repository)
        .stdin(process::Stdio::null())
        .stderr(process::Stdio::inherit())
        .output()?;
    if !output.status.success() {
        return Err(Error::Git(format!("`git branch --merged` returned {}",
                                      output.status)));
    }
    let mut refs = Vec::new();
    for line in output.stdout.split(|&b| b == b'\n') {
        let line = std::str::from_utf8(line)
            .map_err(|_| Error::git("Non-utf8 branch name"))?.trim();
        if line.is_empty().not() {
            refs.push(line.into());
        }
    }
    Ok(refs)
}

pub fn including_branches(
    repository: &Path,
    target: &str,
) -> Result<Vec<String>, Error> {
    let output = process::Command::new("git")
        .args(&["branch", "--contains", target])
        .current_dir(repository)
        .stdin(process::Stdio::null())
        .stderr(process::Stdio::inherit())
        .output()?;
    if !output.status.success() {
        return Err(Error::Git(format!("`git branch --contains` returned {}",
                                      output.status)));
    }
    let mut refs = Vec::new();
    for line in output.stdout.split(|&b| b == b'\n') {
        let line = std::str::from_utf8(line)
            .map_err(|_| Error::git("Non-utf8 branch name"))?.trim();
        if line.is_empty().not() {
            refs.push(line.into());
        }
    }
    Ok(refs)
}

pub fn delete_branch(repository: &Path, name: &str) -> Result<(), Error> {
    let status = process::Command::new("git")
        .args(&["branch", "-D", name])
        .current_dir(repository)
        .stdin(process::Stdio::null())
        .status()?;
    if !status.success() {
        return Err(Error::Git(format!("`git branch -D` returned {}", status)));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::{Operation, Ref};
    use crate::git::{parse_operation, parse_fetch_output};

    #[test]
    fn test_parse_operation() {
        assert!(parse_operation("").is_err());
        assert!(parse_operation("++").is_err());
        assert_eq!("\u{E9}".len(), 2);
        assert!(parse_operation("\u{E9}").is_err());
        assert_eq!(parse_operation("+").unwrap(), Operation::Forced);
    }

    #[test]
    fn test_parse_fetch() {
        let stderr: &[u8] = b"
Fetching origin
remote: Enumerating objects: 14, done.
remote: Counting objects: 100% (14/14), done.
remote: Compressing objects: 100% (11/11), done.
remote: Total 14 (delta 3), reused 12 (delta 1), pack-reused 0
Unpacking objects: 100% (14/14), done.
From github.com:remram44/doublegit
 * [new branch]      master     -> origin/master
   673b728..466e90b  devel      -> origin/devel
 - [deleted]         (none)     -> origin/old
";
        let output = parse_fetch_output(stderr).unwrap();
        assert_eq!(
            output.new,
            [
                Ref {
                    remote: "origin".into(),
                    name: "master".into(),
                    tag: false,
                },
            ].iter().cloned().collect(),
        );
        assert_eq!(
            output.changed,
            [
                Ref {
                    remote: "origin".into(),
                    name: "devel".into(),
                    tag: false,
                },
            ].iter().cloned().collect(),
        );
        assert_eq!(
            output.removed,
            [
                Ref {
                    remote: "origin".into(),
                    name: "old".into(),
                    tag: false,
                },
            ].iter().cloned().collect(),
        );
    }
}
