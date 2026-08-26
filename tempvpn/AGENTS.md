# Repository instructions

## `deploymaster` is local-only

When the current Git branch is `deploymaster`, never run `git push` or any
other command that creates, updates, or deletes a remote Git ref. Commits on
`deploymaster` must remain local.

If work from `deploymaster` needs to be published, stop and ask the user to
switch to or create a different branch first. Do not bypass this rule by
pushing a commit hash, using another remote URL, or changing the branch name
only for the push.

This restriction applies to Git remotes only. It does not prohibit an
explicitly requested Terraform or GCP deployment.
