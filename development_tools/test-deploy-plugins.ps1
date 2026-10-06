$ErrorActionPreference = 'Stop'
. "$PSScriptRoot/deploy-plugins.ps1"

function Assert-True {
    param([bool]$Value, [string]$Message)
    if (-not $Value) { throw $Message }
}

$temporary = Join-Path $env:LOCALAPPDATA ('Temp/opencode/deployment-test-' + [guid]::NewGuid())
New-Item -ItemType Directory -Path $temporary | Out-Null
try {
    $profile = Join-Path $temporary 'profile'
    $cache = Join-Path $profile 'plugins/cache/civilized-agent-local/civilized-agent/0.3.0'
    $source = Join-Path $temporary 'source'
    $shared = Join-Path $temporary 'native-announcer'
    foreach ($directory in @("$cache/.claude-plugin", "$cache/bin", "$source/.claude-plugin", "$source/hooks", "$source/scripts", "$shared/resources")) {
        New-Item -ItemType Directory -Path $directory -Force | Out-Null
    }
    '{"name":"civilized-agent","version":"0.3.0"}' | Set-Content "$cache/.claude-plugin/plugin.json"
    '{"name":"civilized-agent","version":"0.3.0"}' | Set-Content "$source/.claude-plugin/plugin.json"
    'old binary' | Set-Content "$cache/bin/obsolete.exe"
    'new hooks' | Set-Content "$source/hooks/register.ts"
    'new runtime' | Set-Content "$source/scripts/runtime.mjs"
    'shared asset' | Set-Content "$shared/resources/keep.txt"
    $registry = @{ version = 2; plugins = @{ $PluginId = @(@{ scope = 'user'; installPath = $cache }, @{ scope = 'local'; installPath = 'not-user' }) } }
    $registry | ConvertTo-Json -Depth 6 | Set-Content "$profile/plugins/installed_plugins.json"
    $installed = @(Get-ClaudeInstallations $profile)
    Assert-True ($installed.Count -eq 1 -and $installed[0].installPath -eq $cache) 'User profile detection failed'

    Deploy-ClaudeFiles $source $cache $profile $shared
    Assert-True ((Get-Content "$cache/hooks/register.ts") -eq 'new hooks') 'Updated hooks were not deployed'
    Assert-True (-not (Test-Path "$cache/bin/obsolete.exe")) 'Obsolete output survived deployment'
    Assert-True (-not (Get-Item "$cache/native-announcer").LinkType) 'Runtime must be a physical copy'
    Assert-True ((Get-Content "$cache/native-announcer/resources/keep.txt") -eq 'shared asset') 'Shared resources are not accessible'
    Assert-True (@(Get-ChildItem (Split-Path $cache -Parent) -Force | Where-Object Name -Like '.civilized-*').Count -eq 0) 'Staging or backup output survived deployment'

    $rejected = $false
    try { Deploy-ClaudeFiles $source $source $profile $shared } catch { $rejected = $true }
    Assert-True $rejected 'Deployment accepted a directory outside the cache'

    $missingSource = Join-Path $temporary 'incomplete'
    New-Item -ItemType Directory -Path "$missingSource/.claude-plugin" -Force | Out-Null
    Copy-Item "$source/.claude-plugin/plugin.json" "$missingSource/.claude-plugin/plugin.json"
    $failed = $false
    try { Deploy-ClaudeFiles $missingSource $cache $profile $shared } catch { $failed = $true }
    Assert-True $failed 'Incomplete source did not fail'
    Assert-True ((Get-Content "$cache/hooks/register.ts") -eq 'new hooks') 'A staging failure changed the installed plugin'

    Deploy-ClaudeFiles $source $cache $profile $shared
    Assert-True (Test-Path "$shared/resources/keep.txt") 'Redeployment removed shared resources through a junction'
    Remove-DeploymentDirectory $cache
    Assert-True (Test-Path "$shared/resources/keep.txt") 'Deployment cleanup removed the shared announcer'
    $script:enableCalls = 0
    $script:pluginEnabled = $true
    function claude {
        if ($args[1] -eq 'list') {
            @(@{ id = $PluginId; scope = 'user'; enabled = $script:pluginEnabled }) | ConvertTo-Json -AsArray
        } elseif ($args[1] -eq 'enable') {
            $script:enableCalls++
        } else { throw 'Unexpected Claude command in deployment test' }
        $global:LASTEXITCODE = 0
    }
    Enable-ClaudePlugin
    Assert-True ($script:enableCalls -eq 0) 'Deployment tried to enable an already enabled plugin'
    $script:pluginEnabled = $false
    Enable-ClaudePlugin
    Assert-True ($script:enableCalls -eq 1) 'Deployment did not enable a disabled plugin'
    $programs = Join-Path $temporary 'programs Ω with spaces'
    $binary = Join-Path $shared 'announcer Ω.exe'
    Install-SettingsShortcut $binary $programs
    $shortcut = Read-NativeShortcut (Join-Path $programs 'Civilized Agent settings.lnk')
    Assert-True ($shortcut.TargetPath -ceq $binary -and $shortcut.Arguments -ceq '--settings') 'Settings shortcut does not preserve the Unicode settings app path'
    Write-Output '8 deployment checks passed.'
} finally {
    $junction = Join-Path $temporary 'profile/plugins/cache/civilized-agent-local/civilized-agent/0.3.0/native-announcer'
    if (Test-Path -LiteralPath $junction) { Remove-Item -LiteralPath $junction -Force }
    Remove-Item -LiteralPath $temporary -Recurse -Force
}
