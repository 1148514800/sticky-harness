<#
.SYNOPSIS
Run one piece of work and report it to Sticky Harness as a harness task.

.DESCRIPTION
A worked example of the bridge, not part of the app. It shows the pattern a
custom harness needs: report the start, run the work, then report the outcome -
including a failure, so a crashed run does not leave a task sitting in the
Harness Tasks window forever.

The work is a script block, which is how PowerShell takes a command with its own
flags without any quoting games: anything inside the braces is passed through
untouched.

.EXAMPLE
.\run-task.ps1 -Harness my-bot -Name "My Bot" -Task "Run the test suite" -Work { npm test }

.EXAMPLE
.\run-task.ps1 -Harness deepseek -Name DeepSeek -Task "Index the repo" -Work { py -3 index.py --all }
#>
param(
  [Parameter(Mandatory = $true)][string]$Harness,
  [Parameter(Mandatory = $true)][string]$Name,
  [Parameter(Mandatory = $true)][string]$Task,
  [Parameter(Mandatory = $true)][scriptblock]$Work
)

$bridge = Join-Path $PSScriptRoot "..\bin\sticky-harness-bridge.mjs"

if (-not (Test-Path -LiteralPath $bridge)) {
  Write-Error "bridge not found at $bridge; run this from the Sticky Harness repo"
  exit 2
}

# Report the start first, so the task appears in the window while the work runs.
node $bridge start --harness $Harness --name $Name --task $Task
if ($LASTEXITCODE -ne 0) {
  Write-Error "could not report the task to Sticky Harness; is the app running?"
  exit 2
}

$exit = 0
try {
  & $Work
  $exit = $LASTEXITCODE
  if ($null -eq $exit) { $exit = 0 }
} catch {
  Write-Error $_
  $exit = 1
}

# Always finish the task, so the window reflects reality either way.
if ($exit -eq 0) {
  node $bridge done
} else {
  node $bridge fail --message "exited with code $exit"
}

exit $exit
