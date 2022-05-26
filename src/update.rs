use std::{path::{PathBuf, Path}, io::{self, Write}, cell::RefCell, fs};

use git2::{Progress, RemoteCallbacks, FetchOptions, build::RepoBuilder, AutotagOption, Repository};

use crate::{config::{RepoConfig, Config}, errors::Result};

// Most of this clone/fetch code is copied from the git2-rs examples

struct State {
    progress: Option<Progress<'static>>,
    total: usize,
    current: usize,
    path: Option<PathBuf>,
    newline: bool,
}

fn print(state: &mut State) {
    let stats = state.progress.as_ref().unwrap();
    let network_pct = (100 * stats.received_objects()) / stats.total_objects();
    let index_pct = (100 * stats.indexed_objects()) / stats.total_objects();
    let co_pct = if state.total > 0 {
        (100 * state.current) / state.total
    } else {
        0
    };
    let kbytes = stats.received_bytes() / 1024;
    if stats.received_objects() == stats.total_objects() {
        if !state.newline {
            println!();
            state.newline = true;
        }
        print!(
            "Resolving deltas {}/{}\r",
            stats.indexed_deltas(),
            stats.total_deltas()
        );
    } else {
        print!(
            "net {:3}% ({:4} kb, {:5}/{:5})  /  idx {:3}% ({:5}/{:5})  \
            /  chk {:3}% ({:4}/{:4}) {}\r",
            network_pct,
            kbytes,
            stats.received_objects(),
            stats.total_objects(),
            index_pct,
            stats.indexed_objects(),
            stats.total_objects(),
            co_pct,
            state.current,
            state.total,
            state
                .path
                .as_ref()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default()
        )
    }
    io::stdout().flush().unwrap();
}

#[tracing::instrument]
fn clone_repository(repo_config: &RepoConfig, path: &Path) -> Result<Repository> {
    let state = RefCell::new(State {
        progress: None,
        total: 0,
        current: 0,
        path: None,
        newline: false,
    });
    let mut cb = RemoteCallbacks::new();
    cb.transfer_progress(|stats| {
        let mut state = state.borrow_mut();
        state.progress = Some(stats.to_owned());
        print(&mut *state);
        true
    });

    let mut fo = FetchOptions::new();
    fo.download_tags(AutotagOption::All);
    fo.remote_callbacks(cb);
    let repo = RepoBuilder::new()
        .fetch_options(fo)
        .bare(true)
        .remote_create(|repo,name,url| repo.remote_with_fetch(name, url, "+refs/*:refs/*"))
        .clone(&repo_config.url, path)?;

    repo.config()?.set_bool("remote.origin.mirror", true)?;

    println!();

    Ok(repo)
}

#[tracing::instrument]
fn fetch_repo(repo_config: &RepoConfig, path: &Path) -> Result<Repository> {
    let repo = Repository::open(path)?;

    {
        let mut cb = RemoteCallbacks::new();
        let mut remote = repo
            .find_remote("origin")
            .or_else(|_| repo.remote_anonymous(&repo_config.url))?;
        cb.sideband_progress(|data| {
            print!("remote: {}", std::str::from_utf8(data).unwrap());
            io::stdout().flush().unwrap();
            true
        });

        // This callback gets called for each remote-tracking branch that gets
        // updated. The message we output depends on whether it's a new one or an
        // update.
        cb.update_tips(|refname, a, b| {
            if a.is_zero() {
                tracing::info!("[new]     {:20} {}", b, refname);
            } else {
                tracing::info!("[updated] {:10}..{:10} {}", a, b, refname);
            }
            true
        });

        // Here we show processed and total objects in the pack and the amount of
        // received data. Most frontends will probably want to show a percentage and
        // the download rate.
        cb.transfer_progress(|stats| {
            if stats.received_objects() == stats.total_objects() {
                print!(
                    "Resolving deltas {}/{}\r",
                    stats.indexed_deltas(),
                    stats.total_deltas()
                );
            } else if stats.total_objects() > 0 {
                print!(
                    "Received {}/{} objects ({}) in {} bytes\r",
                    stats.received_objects(),
                    stats.total_objects(),
                    stats.indexed_objects(),
                    stats.received_bytes()
                );
            }
            io::stdout().flush().unwrap();
            true
        });

        // Download the packfile and index it. This function updates the amount of
        // received data and the indexer stats which lets you inform the user about
        // progress.
        let mut fo = FetchOptions::new();
        fo.remote_callbacks(cb);
        remote.download(&[] as &[&str], Some(&mut fo))?;

        {
            // If there are local objects (we got a thin pack), then tell the user
            // how many objects we saved from having to cross the network.
            let stats = remote.stats();
            if stats.local_objects() > 0 {
                tracing::info!(
                    "Received {}/{} objects in {} bytes (used {} local \
                    objects)",
                    stats.indexed_objects(),
                    stats.total_objects(),
                    stats.received_bytes(),
                    stats.local_objects()
                );
            } else {
                tracing::info!(
                    "Received {}/{} objects in {} bytes",
                    stats.indexed_objects(),
                    stats.total_objects(),
                    stats.received_bytes()
                );
            }
        }

        // Disconnect the underlying connection to prevent from idling.
        remote.disconnect()?;

        // Update the references in the remote's namespace to point to the right
        // commits. This may be needed even if there was no packfile to download,
        // which can happen e.g. when the branches have been changed but all the
        // needed objects are available locally.
        remote.update_tips(None, true, AutotagOption::Unspecified, None)?;
    }

    Ok(repo)
}

fn update_refs_info(repo: &Repository) -> Result<()> {
    let mut output = String::new();
    for rf in repo.references()? {
        let rf = rf?;
        if let Some(target) = rf.target() {
            output.push_str(&format!("{}\t{}\n", target, rf.name().unwrap()));
        }
    }

    let dir = repo.path().join("info");
    if !dir.exists() {
        fs::create_dir_all(&dir)?;
    }
    let refs_info = dir.join("refs");
    fs::write(refs_info, output)?;

    Ok(())
}

fn update_head(config: &RepoConfig, repo: &Repository) -> Result<()> {
    repo.set_head(&format!("refs/heads/{}", config.head))?;
    Ok(())
}

pub(crate) fn update_repos(config: Config) -> Result<()> {
    for (slug, repo_config) in config.repos {
        let mut path = PathBuf::new();
        path.push("repos");
        path.push(format!("{}.git", slug));
        let repo = if !path.exists() {
            tracing::info!("Cloning {} into {:?}...", repo_config.url, &path);
            clone_repository(&repo_config, &path)?
        } else {
            tracing::info!("Fetching {} in {:?}...", repo_config.url, &path);
            fetch_repo(&repo_config, &path)?
        };
        update_refs_info(&repo)?;
        update_head(&repo_config, &repo)?;
    }

    Ok(())
}
