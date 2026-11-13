#!/usr/bin/env bash
# Put the project on GitHub (first time) or push what's changed since (any time after).
#   ./publish-github.sh                          # push changes (asks for a commit message)
#   ./publish-github.sh -m "Fix the doors"       # with the message given
#   ./publish-github.sh --repo git@github.com:you/open-ultima8.git   # the first time: where to
#   ./publish-github.sh --create open-ultima8 [--private]           # or have gh make the repo
#   ./publish-github.sh --tag v0.1.0             # also tag a release (Actions builds and attaches
#                                                #   the Linux, Windows and macOS packages)
# Tidies up first: the workflow into .github/workflows/, the GPL-3.0 LICENSE if missing, and
# .gitignore entries for what mustn't go up (the game data in U8/, builds, screenshots).
# Commits credit Claude as co-author (so it shows under Contributors).
set -euo pipefail
cd "$(dirname "$0")"

msg="" repo="" create="" visibility="--public" tag=""
while [[ $# -gt 0 ]]; do
    case $1 in
        -m) msg=$2; shift ;;
        --repo) repo=$2; shift ;;
        --create) create=$2; shift ;;
        --private) visibility="--private" ;;
        --tag) tag=$2; shift ;;
        -h | --help) sed -n '2,10p' "$0"; exit ;;
        *) echo "unknown option: $1 (see --help)" >&2; exit 2 ;;
    esac
    shift
done

die() { echo "!! $*" >&2; exit 1; }
say() { echo "== $*"; }
command -v git >/dev/null || die "needs git (sudo dnf install git)"

# --- tidy up ---------------------------------------------------------------------------
if [[ -f ci.yml ]]; then
    mkdir -p .github/workflows
    mv -f ci.yml .github/workflows/ci.yml
    say "moved ci.yml → .github/workflows/ci.yml"
fi
if [[ ! -f LICENSE ]]; then
    curl -fsSL https://www.gnu.org/licenses/gpl-3.0.txt -o LICENSE || die "couldn't download the GPL-3.0 text into LICENSE"
    say "added LICENSE (GPL-3.0)"
fi
touch .gitignore
for p in /target /out /dist /U8/ /u8/ "/Claude outputs/" "*.wav"; do
    grep -qxF "$p" .gitignore || { echo "$p" >> .gitignore; say "ignoring $p"; }
done

# --- the repository --------------------------------------------------------------------
if [[ ! -d .git ]]; then
    git init -q -b main
    say "new git repository (branch main)"
fi
git config user.name >/dev/null || die "tell git who you are: git config --global user.name \"Your Name\""
git config user.email >/dev/null || die "and: git config --global user.email you@example.com"

git add -A
# The game's files must never go up: stop if any slipped past .gitignore.
if git diff --cached --name-only | grep -Eiq '(^|/)(U8|u8)/|\.(flx|dat|skf)$|U8SAVE\.|U8\.EXE'; then
    git diff --cached --name-only | grep -Ei '(^|/)(U8|u8)/|\.(flx|dat|skf)$|U8SAVE\.|U8\.EXE' | head
    git reset -q
    die "game data is about to be committed (above) — not pushing. Check .gitignore."
fi
if git diff --cached --quiet; then
    say "nothing new to commit"
else
    git diff --cached --stat | tail -1
    if [[ -z $msg ]]; then
        if git rev-parse -q --verify HEAD >/dev/null; then
            read -rp "Commit message: " msg
        else
            msg="Open Ultima8: first upload"
        fi
    fi
    [[ -n $msg ]] || die "no commit message"
    git commit -q -m "$msg" -m "Co-authored-by: Claude <noreply@anthropic.com>"
    say "committed: $msg"
fi

# --- where it goes ---------------------------------------------------------------------
if ! git remote get-url origin >/dev/null 2>&1; then
    if [[ -n $repo ]]; then
        git remote add origin "$repo"
    elif [[ -n $create ]]; then
        command -v gh >/dev/null || die "--create needs the GitHub CLI: sudo dnf install gh, then gh auth login"
        gh repo create "$create" "$visibility" --source . --remote origin \
            --description "Open-source Ultima VIII: Pagan client in Rust, with sprite upscaling"
    else
        die "where to? Make an empty repo on github.com and run again with --repo <its URL>, or use --create <name>"
    fi
    say "remote: $(git remote get-url origin)"
fi

branch=$(git branch --show-current)
git push -u origin "$branch"
say "pushed $branch"

if [[ -n $tag ]]; then
    [[ $tag == v* ]] || die "tags start with v (v0.1.0): that's what makes Actions build a release"
    git tag -a "$tag" -m "Open Ultima8 $tag"
    git push origin "$tag"
    say "tagged $tag: the release appears under Releases once Actions has built the packages"
fi
url=$(git remote get-url origin | sed -E 's#^git@github.com:#https://github.com/#; s#\.git$##')
say "done: $url (builds: $url/actions)"
