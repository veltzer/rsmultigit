//! Git remote URLs taken apart, in one place. `check submodules` compares
//! submodule URLs by identity, and the GitHub commands need the owner/name
//! a remote points at; both go through [`GitUrl::parse`], so every URL form
//! (`https://`, `ssh://` with a port, scp-style `git@host:path`, bare
//! `host/path`, relative paths) is understood the same way everywhere.

/// A remote URL reduced to what identifies the repository: the host, with
/// any user and port dropped, and the path, without surrounding slashes or
/// a trailing `.git`. A local or relative URL (`../themes`, `/srv/repo`)
/// has no host and keeps its path as written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitUrl {
    pub host: Option<String>,
    pub path: String,
}

impl GitUrl {
    pub fn parse(url: &str) -> Self {
        let url = url.trim();
        if url.starts_with('.') || url.starts_with('/') {
            return Self {
                host: None,
                path: strip_git_suffix(url).to_string(),
            };
        }
        let (authority, path) = if let Some((_, rest)) = url.split_once("://") {
            // scheme://[user@]host[:port]/path
            rest.split_once('/').unwrap_or((rest, ""))
        } else {
            match url.split_once(':') {
                // scp-style [user@]host:path. A colon after the first slash
                // belongs to the path, not to scp syntax.
                Some((authority, path)) if !authority.contains('/') => (authority, path),
                // Bare host/path, as a rule may spell it.
                _ => url.split_once('/').unwrap_or((url, "")),
            }
        };
        let host = authority.rsplit('@').next().unwrap_or(authority);
        let host = host.split(':').next().unwrap_or(host);
        Self {
            host: Some(host.to_ascii_lowercase()),
            path: strip_git_suffix(path.trim_matches('/')).to_string(),
        }
    }

    /// The repository's identity for comparing two URLs: `host/path`, case
    /// folded, so every spelling of one remote compares equal.
    pub fn identity(&self) -> String {
        match &self.host {
            Some(host) => format!("{host}/{}", self.path).to_ascii_lowercase(),
            None => self.path.to_ascii_lowercase(),
        }
    }

    /// The GitHub repository this URL names, when it is a github.com URL
    /// of exactly `owner/name`.
    pub fn github_repo(&self) -> Option<RepoId> {
        let host = self.host.as_deref()?;
        if host != "github.com" && host != "www.github.com" {
            return None;
        }
        RepoId::from_slug(&self.path)
    }
}

/// Drop a trailing `.git` (any case) and any slash before it.
fn strip_git_suffix(path: &str) -> &str {
    let path = path.trim_end_matches('/');
    let cut = path.len().saturating_sub(4);
    match path.get(cut..) {
        Some(tail) if tail.eq_ignore_ascii_case(".git") => path[..cut].trim_end_matches('/'),
        _ => path,
    }
}

/// A GitHub repository, as owner and name.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RepoId {
    pub owner: String,
    pub name: String,
}

impl RepoId {
    /// `owner/name` (a trailing `/` or `.git` tolerated), when both parts
    /// are valid GitHub names. Anything else, including a longer path such
    /// as `owner/name/tree/main`, is not a repository.
    pub fn from_slug(slug: &str) -> Option<Self> {
        let slug = strip_git_suffix(slug);
        let (owner, name) = slug.split_once('/')?;
        (is_github_name(owner) && is_github_name(name)).then(|| Self {
            owner: owner.to_string(),
            name: name.to_string(),
        })
    }

    pub fn slug(&self) -> String {
        format!("{}/{}", self.owner, self.name)
    }
}

/// GitHub owner and repository names are made of these. Checking this also
/// makes a name safe to put inside a GraphQL string literal or a URL path.
fn is_github_name(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity(url: &str) -> String {
        GitUrl::parse(url).identity()
    }

    #[test]
    fn every_spelling_of_one_remote_has_one_identity() {
        let expected = "github.com/veltzer/shared-themes";
        for url in [
            "git@github.com:veltzer/shared-themes.git",
            "https://github.com/veltzer/shared-themes",
            "https://github.com/veltzer/shared-themes.git/",
            "ssh://git@github.com/veltzer/shared-themes.git",
            "ssh://git@github.com:22/veltzer/shared-themes.git",
            "git+ssh://git@github.com/Veltzer/Shared-Themes",
            "GitHub.com/veltzer/shared-themes",
            "github.com/veltzer/shared-themes.GIT",
        ] {
            assert_eq!(identity(url), expected, "{url}");
        }
    }

    #[test]
    fn relative_and_local_urls_have_no_host() {
        let url = GitUrl::parse("../shared-themes.git");
        assert_eq!(url.host, None);
        assert_eq!(url.identity(), "../shared-themes");
        assert_eq!(GitUrl::parse("/srv/git/repo").identity(), "/srv/git/repo");
    }

    #[test]
    fn a_colon_inside_the_path_is_not_scp_syntax() {
        assert_eq!(
            identity("https://host/owner/repo:with:colons"),
            "host/owner/repo:with:colons"
        );
    }

    fn id(o: &str, n: &str) -> RepoId {
        RepoId {
            owner: o.to_string(),
            name: n.to_string(),
        }
    }

    #[test]
    fn github_repo_from_every_url_form() {
        for url in [
            "git@github.com:veltzer/rsmultigit.git",
            "git@github.com:veltzer/rsmultigit",
            "https://github.com/veltzer/rsmultigit",
            "https://github.com/veltzer/rsmultigit.git",
            "https://github.com/veltzer/rsmultigit/",
            "https://user@github.com/veltzer/rsmultigit.git",
            "https://www.github.com/veltzer/rsmultigit",
            "ssh://git@github.com/veltzer/rsmultigit.git",
            "ssh://git@github.com:22/veltzer/rsmultigit.git",
            "git://github.com/veltzer/rsmultigit.git",
        ] {
            assert_eq!(
                GitUrl::parse(url).github_repo(),
                Some(id("veltzer", "rsmultigit")),
                "{url}"
            );
        }
    }

    #[test]
    fn github_repo_rejects_everything_else() {
        for url in [
            "git@gitlab.com:veltzer/rsmultigit.git",
            "https://gitlab.com/veltzer/rsmultigit",
            "https://github.com.evil.example/veltzer/rsmultigit",
            "https://gist.github.com/veltzer/abc",
            "https://github.com/veltzer",
            "https://github.com/veltzer/rsmultigit/tree/master",
            "/home/mark/git/rsmultigit",
            "https://github.com/ve\"ltzer/x",
        ] {
            assert_eq!(GitUrl::parse(url).github_repo(), None, "{url}");
        }
    }

    #[test]
    fn slug_round_trips() {
        assert_eq!(RepoId::from_slug("o/n.git"), Some(id("o", "n")));
        assert_eq!(id("o", "n").slug(), "o/n");
        assert_eq!(RepoId::from_slug("o/n/x"), None);
        assert_eq!(RepoId::from_slug("o"), None);
    }
}
