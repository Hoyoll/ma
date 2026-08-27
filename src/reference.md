%H   full commit hash
%h   abbreviated hash
%an  author name
%ae  author email
%aI  author date, strict ISO 8601
%cn  committer name
%cI  committer date, strict ISO 8601
%s   subject
%b   body
%B   raw body
%P   parent hashes
%T   tree hash

# Branch: <code for the lsp to read on change> 
-> <then after did_change method read this code it delete it again> 
-> <ofc there's in memory repr over WHAT that code mean>
$GIT_DIR //ie the /.git
    /ma-cache
        /<commit-hash>
            /
            root.diff
            <files from that hash>
