# Gitit

A simple repository browser and mirrorer for Git.

## Configuration

Gitit uses a single `toml` file named `gitit.toml` for its configuration, and the contents look something like this:

```toml
[server]
address = "0.0.0.0:3000"

[repos.gitit]
url = "https://github.com/ashhhleyyy/gitit.git"
title = "Gitit"

[repos.website]
url = "https://github.com/ashhhleyyy/website.git"
title = "Website"
```

In order to keep the repositories in sync with their upstream, you should call `gitit update-repos` on a regular schedule, or even automate it with webhooks (instructions not included).

To run the web server, run `gitit web`.
