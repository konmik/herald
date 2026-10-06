$ErrorActionPreference = 'Stop'
. "$PSScriptRoot/bundle/install.ps1"

function Assert-True {
    param([bool]$Value, [string]$Message)
    if (-not $Value) { throw $Message }
}

function Assert-Rejected {
    param([scriptblock]$Action, [string]$Message)
    $rejected = $false
    try { & $Action | Out-Null } catch { $rejected = $true }
    Assert-True $rejected $Message
}

function Assert-SettingsPreserved {
    param([string]$Path, [string]$Expected, [string]$Message)
    Assert-True ([Convert]::ToHexString([IO.File]::ReadAllBytes($Path)) -ceq $Expected) $Message
}

$temporary = Join-Path $env:LOCALAPPDATA ('Temp/opencode/Civilized Agent Ω bundle-test-' + [guid]::NewGuid())
New-Item -ItemType Directory -Path $temporary | Out-Null
$previousCivilizedAgentData = $env:CIVILIZED_AGENT_DATA
$nativeData = Join-Path $temporary 'native-data'
$env:CIVILIZED_AGENT_DATA = $nativeData
try {
    New-Item -ItemType Directory -Path $nativeData | Out-Null
    $settingsPath = Join-Path $nativeData 'settings.json'
    $settingsJson = @'
{
  "quietMode": true,
  "scheduleEnabled": true,
  "quietStart": 1350,
  "quietEnd": 495,
  "volume": 35,
  "outputDevice": "saved-output-device",
  "elevenlabsApiKey": "saved-api-key",
  "speechModel": "eleven_v4_turbo",
  "voices": {
    "claude": "Mark",
    "opencode": "Luna"
  },
  "characters": {
    "saved-herald": {
      "name": "Saved herald",
      "voiceDescription": "A warm saved voice",
      "animationPath": "C:\\Videos\\saved-herald.mp4",
      "voice": {
        "type": "elevenLabs",
        "voiceId": "saved-voice"
      }
    }
  },
  "selectedCharacter": "saved-herald",
  "installedBundledCharacters": []
}
'@
    [IO.File]::WriteAllText($settingsPath, $settingsJson, [Text.UTF8Encoding]::new($false))
    $settingsBytes = [Convert]::ToHexString([IO.File]::ReadAllBytes($settingsPath))
    $payload = Join-Path $temporary 'payload'
    $arch = if ([Runtime.InteropServices.RuntimeInformation]::OSArchitecture -eq 'Arm64') { 'arm64' } else { 'x64' }
    $required = @('claude-plugin/.claude-plugin/plugin.json', 'claude-plugin/.claude-plugin/marketplace.json', 'claude-plugin/hooks/register.ts', 'claude-plugin/hooks/hooks.json', 'claude-plugin/scripts/bridge.mjs', 'claude-plugin/scripts/runtime.mjs', 'claude-plugin/scripts/session-title.mjs', 'claude-plugin/native-announcer/bin/node.exe')
    foreach ($prefix in @('native-announcer', 'claude-plugin/native-announcer')) {
        $required += "$prefix/bin/civilized-announcer-win32-$arch.exe", "$prefix/bin/onnxruntime.dll", "$prefix/bin/sherpa-onnx-c-api.dll", "$prefix/resources/characters.json", "$prefix/resources/videos/fixture.mp4"
        foreach ($name in @('model.int8.onnx', 'voices.bin', 'tokens.txt', 'espeak-ng-data/en_dict', 'LICENSE')) { $required += "$prefix/resources/tts/kitten-nano-en-v0_8-int8/$name" }
    }
    foreach ($file in $required) {
        $path = Join-Path $payload $file
        New-Item -ItemType Directory -Path (Split-Path $path -Parent) -Force | Out-Null
        'fixture' | Set-Content -LiteralPath $path
    }
    '{"name":"civilized-agent","version":"0.3.0"}' | Set-Content "$payload/claude-plugin/.claude-plugin/plugin.json"
    '{"name":"civilized-agent-local"}' | Set-Content "$payload/claude-plugin/.claude-plugin/marketplace.json"
    foreach ($prefix in @('native-announcer', 'claude-plugin/native-announcer')) { '{"fixture":{"animationPath":"videos/fixture.mp4"}}' | Set-Content "$payload/$prefix/resources/characters.json" }
    Copy-Item -LiteralPath (Join-Path (Split-Path $PSScriptRoot -Parent) 'claude-plugin/hooks/register.ts') -Destination "$payload/claude-plugin/hooks/register.ts" -Force
    $zip = & "$PSScriptRoot/build-bundle.ps1" -PayloadDirectory $payload -OutputDirectory (Join-Path $temporary 'output')
    $bundle = Join-Path $temporary 'extracted'
    Expand-Archive -LiteralPath $zip -DestinationPath $bundle
    Test-Bundle $bundle | Out-Null
    Assert-True (-not (Test-Path "$bundle/node_modules")) 'Compiled bundle contains a dependency tree'
    foreach ($package in @('@opencode-plugin-2.0.24', 'effect-4.0.0-rc.112', 'jsonc-parser-3.3.1')) {
        Assert-True (@(Get-ChildItem -LiteralPath "$bundle/licenses/javascript/$package" -File).Count -gt 0) "Bundled JavaScript license missing: $package"
    }
    $entry = [uri]::new((Join-Path $bundle 'index.ts')).AbsoluteUri
    & bun -e "const {default: plugin} = await import('$entry'); if (plugin.id !== 'civilized-agent') throw new Error('Wrong packaged plugin')"
    Assert-True ($LASTEXITCODE -eq 0) 'Compiled plugin failed to import outside the checkout'
    $tuiEntry = [uri]::new((Join-Path $bundle 'tui.ts')).AbsoluteUri
    $peerDirectory = Join-Path $temporary 'node_modules'
    $solidPeer = Join-Path $peerDirectory 'solid-js'
    New-Item -ItemType Directory -Path $peerDirectory | Out-Null
    New-Item -ItemType Junction -Path $solidPeer -Target (Join-Path (Split-Path $PSScriptRoot -Parent) 'node_modules/solid-js') | Out-Null
    $solidEntry = [uri]::new((Join-Path $solidPeer 'dist/solid.js')).AbsoluteUri
    $tuiScript = @"
import { readdir, readFile } from 'node:fs/promises';
import { join } from 'node:path';
const {default: plugin} = await import('$tuiEntry');
if (plugin.id !== 'civilized-agent.tui' || typeof plugin.setup !== 'function') throw new Error('Wrong packaged TUI plugin');
const {createRoot} = await import('$solidEntry');
const inbox = join(process.env.CIVILIZED_AGENT_DATA, 'inbox');
async function waitForPresence(matches) {
    const deadline = Date.now() + 5000;
    while (Date.now() < deadline) {
        const files = await readdir(inbox).catch((error) => { if (error.code === 'ENOENT') return []; throw error });
        const messages = await Promise.all(files.filter((file) => file.endsWith('.json')).map(async (file) => JSON.parse(await readFile(join(inbox, file), 'utf8'))));
        if (messages.some(matches)) return;
        await Bun.sleep(10);
    }
    throw new Error('Timed out waiting for packaged TUI presence');
}
let dispose;
try {
    createRoot((rootDispose) => {
        dispose = rootDispose;
        plugin.setup({ui: {slot({render}) { render() }, router: {current: () => ({type: 'session', sessionID: 'root'})}, tabs: {enabled: () => true, list: () => [{sessionID: 'root'}]}}});
    });
    await waitForPresence((message) => message.type === 'presence' && message.sessionIDs?.[0] === 'root');
} finally { dispose?.() }
await waitForPresence((message) => message.type === 'presence' && Array.isArray(message.sessionIDs) && message.sessionIDs.length === 0);
"@
    try {
        & bun --conditions=browser -e $tuiScript
        Assert-True ($LASTEXITCODE -eq 0) 'Packaged TUI plugin failed to import and render outside the checkout'
    } finally {
        Remove-Item -LiteralPath $solidPeer -Force
        Remove-Item -LiteralPath $peerDirectory -Force
    }
    $presence = @(Get-ChildItem -LiteralPath (Join-Path $nativeData 'inbox') -Filter '*.json' -File | ForEach-Object { Get-Content -LiteralPath $_.FullName -Raw | ConvertFrom-Json })
    $initial = @($presence | Where-Object { $_.type -eq 'presence' -and $_.sequence -eq 1 }) | Select-Object -First 1
    $cleared = @($presence | Where-Object { $_.type -eq 'presence' -and $_.sessionIDs -is [array] -and $_.sessionIDs.Count -eq 0 }) | Select-Object -First 1
    Assert-True ($null -ne $initial -and $initial.clientID -and $initial.sessionIDs -is [array] -and $initial.sessionIDs.Count -eq 1 -and $initial.sessionIDs[0] -eq 'root' -and [long]$initial.at -gt 0) 'Packaged TUI plugin did not send initial presence'
    Assert-True ($null -ne $cleared -and $cleared.clientID -eq $initial.clientID -and $cleared.sequence -gt $initial.sequence -and [long]$cleared.at -gt 0) 'Packaged TUI plugin did not clear presence'
    $sourceManifest = [IO.File]::ReadAllBytes("$bundle/bundle-manifest.json")
    $repacked = & "$PSScriptRoot/build-bundle.ps1" -PayloadDirectory $bundle -OutputDirectory (Join-Path $temporary 'repacked-output')
    $repackedBundle = Join-Path $temporary 'repacked'
    Expand-Archive -LiteralPath $repacked -DestinationPath $repackedBundle
    Test-Bundle $repackedBundle | Out-Null
    Assert-True ([Convert]::ToHexString([IO.File]::ReadAllBytes("$bundle/bundle-manifest.json")) -eq [Convert]::ToHexString($sourceManifest)) 'Repack changed the source manifest'
    Assert-True ((Get-Content "$repackedBundle/shortcut.ps1" -Raw) -ceq (Get-Content "$PSScriptRoot/bundle/shortcut.ps1" -Raw)) 'Repack did not refresh the shortcut adapter'
    Assert-True ((Get-Content "$bundle/claude-plugin/hooks/register.ts" -Raw) -ceq (Get-Content "$PSScriptRoot/../claude-plugin/hooks/register.ts" -Raw)) 'Packaging rewrote the Claude hook source'
    $root = Join-Path $temporary 'installed'
    & "$bundle/install.ps1" -Bundle $bundle -InstallDirectory $root -SkipHostRegistration -NoStart -WhatIf | Out-Null
    Assert-True (-not (Test-Path $root)) 'WhatIf mutated the installation'
    $app = & "$bundle/install.ps1" -Bundle $bundle -InstallDirectory $root -SkipHostRegistration -NoStart
    Assert-True (Test-Path "$app/native-announcer/resources/videos/fixture.mp4") 'Runtime asset was not installed'
    Assert-True (-not (Get-Item "$app/claude-plugin/native-announcer").LinkType) 'Claude runtime is linked'
    Assert-SettingsPreserved $settingsPath $settingsBytes 'First installation changed native settings'
    $again = & "$bundle/install.ps1" -Bundle $bundle -InstallDirectory $root -SkipHostRegistration -NoStart
    Assert-True ($again -eq $app) 'Repeated installation changed the immutable location'
    Assert-SettingsPreserved $settingsPath $settingsBytes 'Repeated installation changed native settings'
    $lock = [IO.File]::Open((Join-Path $root '.install.lock'), 'Open', 'ReadWrite', 'None')
    try { Assert-Rejected { Install-Payload $bundle $root } 'Concurrent installation acquired the same root' } finally { $lock.Dispose() }
    'corrupt' | Set-Content "$app/native-announcer/bin/onnxruntime.dll"
    $repair = & "$bundle/install.ps1" -Bundle $bundle -InstallDirectory $root -SkipHostRegistration -NoStart
    Assert-True ($repair -eq $app -and (Get-Content "$app/native-announcer/bin/onnxruntime.dll") -eq 'fixture') 'Corrupt owned runtime was not repaired'
    Assert-SettingsPreserved $settingsPath $settingsBytes 'Repair changed native settings'
    $file = "$bundle/native-announcer/bin/onnxruntime.dll"
    $bytes = [IO.File]::ReadAllBytes($file)
    'tampered' | Set-Content $file
    $untouched = Join-Path $temporary 'untouched'
    Assert-Rejected { & "$bundle/install.ps1" -Bundle $bundle -InstallDirectory $untouched -SkipHostRegistration -NoStart } 'Tampered payload was accepted'
    Assert-True (-not (Test-Path $untouched)) 'Tampered payload mutated the destination'
    [IO.File]::WriteAllBytes($file, $bytes)
    Remove-Item $file
    Assert-Rejected { Test-Bundle $bundle } 'Missing payload was accepted'
    [IO.File]::WriteAllBytes($file, $bytes)
    $manifestPath = "$bundle/bundle-manifest.json"
    $original = [IO.File]::ReadAllText($manifestPath)
    foreach ($modulePath in @('index.ts', 'tui.ts', 'opencode-plugin/index.js', 'opencode-plugin/tui.js')) {
        $moduleBytes = [IO.File]::ReadAllBytes((Join-Path $bundle $modulePath))
        Remove-Item -LiteralPath (Join-Path $bundle $modulePath)
        $manifest = $original | ConvertFrom-Json
        $manifest.files = @($manifest.files | Where-Object path -NE $modulePath)
        $manifest | ConvertTo-Json -Depth 8 | Set-Content $manifestPath
        Assert-Rejected { Test-Bundle $bundle } "Missing production entry was accepted: $modulePath"
        [IO.File]::WriteAllBytes((Join-Path $bundle $modulePath), $moduleBytes)
        [IO.File]::WriteAllText($manifestPath, $original)
    }
    $manifest = $original | ConvertFrom-Json
    $manifest.files[0].path = '../outside'
    $manifest | ConvertTo-Json -Depth 8 | Set-Content $manifestPath
    Assert-Rejected { Test-Bundle $bundle } 'Traversal path was accepted'
    [IO.File]::WriteAllText($manifestPath, $original)
    $manifest = $original | ConvertFrom-Json
    $manifest.files += $manifest.files[0]
    $manifest | ConvertTo-Json -Depth 8 | Set-Content $manifestPath
    Assert-Rejected { Test-Bundle $bundle } 'Duplicate payload was accepted'
    [IO.File]::WriteAllText($manifestPath, $original)
    $manifest = $original | ConvertFrom-Json
    $manifest.arch = 'unsupported'
    $manifest | ConvertTo-Json -Depth 8 | Set-Content $manifestPath
    Assert-Rejected { Test-Bundle $bundle } 'Wrong architecture was accepted'
    [IO.File]::WriteAllText($manifestPath, $original)
    New-Item -ItemType Junction -Path "$bundle/linked" -Target $payload | Out-Null
    Assert-Rejected { Test-Bundle $bundle } 'Bundle junction was accepted'
    Remove-Item -LiteralPath "$bundle/linked" -Force
    $foreign = Join-Path $temporary 'foreign'
    New-Item -ItemType Junction -Path $foreign -Target $root | Out-Null
    Assert-Rejected { Install-Payload $bundle $foreign } 'Installation through a junction was accepted'
    Remove-Item -LiteralPath $foreign -Force
    $profile = Join-Path $temporary 'claude'
    $cache = Join-Path $profile 'plugins/cache/civilized-agent-local/civilized-agent/0.3.0'
    New-Item -ItemType Directory -Path "$cache/.claude-plugin" -Force | Out-Null
    Copy-Item "$payload/claude-plugin/.claude-plugin/plugin.json" "$cache/.claude-plugin/plugin.json"
    New-Item -ItemType Junction -Path "$cache/native-announcer" -Target "$payload/native-announcer" | Out-Null
    Deploy-ClaudeFiles "$app/claude-plugin" $cache $profile
    Assert-True (-not (Get-Item "$cache/native-announcer").LinkType) 'Existing Claude junction was not migrated'
    Assert-True (Test-Path "$payload/native-announcer/resources/videos/fixture.mp4") 'Migration deleted the junction target'
    $oldMarketplace = Join-Path $temporary 'old-marketplace'
    New-Item -ItemType Directory -Path "$oldMarketplace/.claude-plugin" -Force | Out-Null
    Copy-Item "$payload/claude-plugin/.claude-plugin/plugin.json" "$oldMarketplace/.claude-plugin/plugin.json"
    '{"name":"civilized-agent-local"}' | Set-Content "$oldMarketplace/.claude-plugin/marketplace.json"
    @{ 'civilized-agent-local' = @{ source = @{ source = 'directory'; path = $oldMarketplace } }; unrelated = @{ source = @{ source = 'github'; repo = 'keep/me' } } } | ConvertTo-Json -Depth 8 | Set-Content "$profile/plugins/known_marketplaces.json"
    @{ version = 2; plugins = @{ $PluginId = @(@{ scope = 'user'; installPath = $cache }) } } | ConvertTo-Json -Depth 8 | Set-Content "$profile/plugins/installed_plugins.json"
    $script:claudeCalls = [Collections.Generic.List[string]]::new()
    function claude {
        $script:claudeCalls.Add(($args -join '|'))
        if ($args[1] -eq 'list') { @(@{ id = $PluginId; scope = 'user'; enabled = $true }) | ConvertTo-Json -AsArray }
        elseif ($args[1] -eq 'marketplace' -and $script:mutateClaude) {
            $registry = Get-Content "$profile/plugins/known_marketplaces.json" -Raw | ConvertFrom-Json -AsHashtable
            $registry['civilized-agent-local'].source.path = $args[3]
            $registry | ConvertTo-Json -Depth 8 | Set-Content "$profile/plugins/known_marketplaces.json"
            '{"unrelated":"changed during installation"}' | Set-Content "$profile/settings.json"
        }
        elseif ($args[1] -ne 'marketplace') { throw 'Unexpected Claude command in bundle test' }
        $global:LASTEXITCODE = 0
    }
    Register-ClaudeBundle $app $profile
    Assert-True ($script:claudeCalls[0] -eq "plugin|marketplace|add|$(Join-Path $app 'claude-plugin')|--scope|user") 'Marketplace migration does not use the durable installed source'
    Assert-True (-not @($script:claudeCalls | Where-Object { $_ -like '*|remove|*' }).Count) 'Marketplace migration removed installed plugins'
    Assert-True ((Get-Content "$profile/plugins/known_marketplaces.json" -Raw | ConvertFrom-Json).unrelated.source.repo -eq 'keep/me') 'Marketplace migration changed unrelated registration data'
    Assert-SettingsPreserved $settingsPath $settingsBytes 'Successful Claude registration changed native settings'
    $openCode = Join-Path $temporary 'opencode'
    New-Item -ItemType Directory -Path $openCode | Out-Null
    $config = Join-Path $openCode 'opencode.jsonc'
    $userConfig = '{/* user version */ "model":"keep","plugins":[]}'
    [IO.File]::WriteAllText($config, $userConfig)
    $writer = [IO.File]::Open($config, 'Open', 'ReadWrite', 'None')
    $script:hostTouched = $false
    try {
        Assert-Rejected { Register-OpenCodeBundle $app @($config) { $script:hostTouched = $true } } 'Exclusive config writer was ignored'
        Assert-True (-not $script:hostTouched) 'Blocked registration changed the host'
    } finally { $writer.Dispose() }
    Assert-True ([IO.File]::ReadAllText($config) -ceq $userConfig) 'Blocked registration overwrote the user version'
    $nodePath = Join-Path $app 'claude-plugin/native-announcer/bin/node.exe'
    $fixtureNode = [IO.File]::ReadAllBytes($nodePath)
    Copy-Item -LiteralPath (Get-Command node.exe).Source -Destination $nodePath -Force
    try {
        $withBom = [byte[]]([Text.Encoding]::UTF8.GetPreamble() + [Text.Encoding]::UTF8.GetBytes($userConfig))
        [IO.File]::WriteAllBytes($config, $withBom)
        Register-OpenCodeBundle $app @($config) {
            Assert-Rejected { [IO.File]::Open($config, 'Open', 'ReadWrite', 'ReadWrite').Dispose() } 'Renderer released the exclusive config lock'
        }
        $registered = [IO.File]::ReadAllBytes($config)
        Assert-True ([Convert]::ToHexString($registered[0..2]) -eq 'EFBBBF') 'Registration lost the UTF-8 BOM'
        Assert-True ([IO.File]::ReadAllText($config).Contains('/* user version */')) 'Registration lost config comments'
        $secondConfig = Join-Path $openCode 'opencode.json'
        [IO.File]::WriteAllText($secondConfig, '{}')
        $secondWriter = [IO.File]::Open($secondConfig, 'Open', 'ReadWrite', 'None')
        $before = [IO.File]::ReadAllBytes($config)
        try {
            Assert-Rejected { Register-OpenCodeBundle $app @($config, $secondConfig) { $script:hostTouched = $true } } 'Second config lock was ignored'
        } finally { $secondWriter.Dispose() }
        Assert-True ([Convert]::ToHexString([IO.File]::ReadAllBytes($config)) -eq [Convert]::ToHexString($before)) 'Lock failure partially changed the first config'
        Assert-True (-not $script:hostTouched) 'Second config lock failure changed the host'
        $utf16 = [byte[]]([Text.Encoding]::Unicode.GetPreamble() + [Text.Encoding]::Unicode.GetBytes($userConfig))
        [IO.File]::WriteAllBytes($config, $utf16)
        Register-OpenCodeBundle $app @($config)
        Assert-True ([Convert]::ToHexString(([IO.File]::ReadAllBytes($config))[0..1]) -eq 'FFFE') 'Registration lost UTF-16 encoding'
        [IO.File]::WriteAllText($config, '{invalid}')
        Assert-Rejected { Register-OpenCodeBundle $app @($config) { $script:hostTouched = $true } } 'Invalid config was accepted'
        Assert-True (-not $script:hostTouched) 'Invalid config changed the host before validation'
        [IO.File]::WriteAllText($config, $userConfig)
        '{"unrelated":"preserve"}' | Set-Content "$profile/settings.json"
        'previous hook' | Set-Content "$cache/hooks/register.ts"
        $successPrograms = Join-Path $temporary 'successful-programs'
        $binary = "$app/native-announcer/bin/civilized-announcer-win32-$arch.exe"
        $script:mutateClaude = $false
        Register-BundleHosts $app @($config) $profile $binary $successPrograms
        Assert-SettingsPreserved $settingsPath $settingsBytes 'Successful host registration changed native settings'
        $rollbackPrograms = Join-Path $temporary 'rollback-programs'
        New-Item -ItemType Directory -Path $rollbackPrograms | Out-Null
        $rollbackShortcut = Join-Path $rollbackPrograms 'Civilized Agent settings.lnk'
        Write-NativeShortcut $rollbackShortcut 'C:/Windows/notepad.exe' '' 'C:/Windows'
        $rollbackPaths = @($config, "$profile/settings.json", "$profile/plugins/known_marketplaces.json", "$profile/plugins/installed_plugins.json", "$cache/hooks/register.ts", $rollbackShortcut)
        $beforeRollback = @{}
        foreach ($path in $rollbackPaths) { $beforeRollback[$path] = [Convert]::ToHexString([IO.File]::ReadAllBytes($path)) }
        $script:mutateClaude = $true
        Assert-Rejected { Register-BundleHosts $app @($config) $profile $binary $rollbackPrograms } 'An unrelated shortcut did not fail host registration'
        foreach ($path in $rollbackPaths) {
            Assert-True ([Convert]::ToHexString([IO.File]::ReadAllBytes($path)) -ceq $beforeRollback[$path]) "Host rollback did not restore $path"
        }
        Assert-SettingsPreserved $settingsPath $settingsBytes 'Host registration rollback changed native settings'
        $absentConfig = Join-Path $openCode 'previously-absent.jsonc'
        Assert-Rejected { Register-BundleHosts $app @($absentConfig) $profile $binary $rollbackPrograms } 'Fresh config rollback did not fail'
        Assert-True (-not (Test-Path -LiteralPath $absentConfig)) 'Rollback left a newly created OpenCode configuration'
        Assert-True (@(Get-ChildItem (Split-Path $cache -Parent) -Force | Where-Object Name -Like '.civilized-*').Count -eq 0) 'Host rollback left a cache backup'
        $script:mutateClaude = $false
    } finally { [IO.File]::WriteAllBytes($nodePath, $fixtureNode) }
    Rename-Item -LiteralPath $bundle -NewName 'source-unavailable'
    Remove-DeploymentDirectory $payload
    Test-Bundle $app | Out-Null
    Assert-True (Test-Path "$cache/native-announcer/bin/node.exe") 'Installed runtime depends on the source'
    $programs = Join-Path $temporary 'programs Ω with spaces'
    Install-SettingsShortcut "$app/native-announcer/bin/civilized-announcer-win32-$arch.exe" $programs
    Install-SettingsShortcut "$app/native-announcer/bin/civilized-announcer-win32-$arch.exe" $programs
    $shortcutPath = Join-Path $programs 'Civilized Agent settings.lnk'
    $shortcut = Read-NativeShortcut $shortcutPath
    $target = [IO.Path]::GetFullPath("$app/native-announcer/bin/civilized-announcer-win32-$arch.exe")
    Assert-True ($shortcut.TargetPath -ceq $target -and $shortcut.Arguments -ceq '--settings' -and $shortcut.WorkingDirectory -ceq (Split-Path $target -Parent)) 'Shortcut did not preserve the exact Unicode target, arguments and working directory'
    Write-NativeShortcut $shortcutPath $target '--other' (Split-Path $target -Parent)
    Assert-Rejected { Install-SettingsShortcut $target $programs } 'Installer overwrote a shortcut with unrelated arguments'
    Assert-True ((Read-NativeShortcut $shortcutPath).Arguments -ceq '--other') 'Argument ownership rejection changed the existing shortcut'
    Write-NativeShortcut "$programs/Unrelated.lnk" 'C:/Windows/notepad.exe' '' 'C:/Windows'
    Write-NativeShortcut $shortcutPath 'C:/Windows/notepad.exe' '--settings' 'C:/Windows'
    Assert-Rejected { Install-SettingsShortcut $target $programs } 'Installer overwrote an unrelated shortcut'
    $foreignBinary = Join-Path $temporary "foreign-app/native-announcer/bin/civilized-announcer-win32-$arch.exe"
    New-Item -ItemType Directory -Path (Split-Path $foreignBinary -Parent) -Force | Out-Null
    'fixture' | Set-Content -LiteralPath $foreignBinary
    '{"name":"unrelated"}' | Set-Content (Join-Path $temporary 'foreign-app/package.json')
    Write-NativeShortcut $shortcutPath $foreignBinary '--settings' (Split-Path $foreignBinary -Parent)
    Assert-Rejected { Install-SettingsShortcut $target $programs } 'Installer trusted an unrelated package with an announcer executable name'
    Assert-True ((Read-NativeShortcut $shortcutPath).TargetPath -ceq $foreignBinary) 'Ownership rejection changed the existing shortcut'
    Assert-True ((Read-NativeShortcut "$programs/Unrelated.lnk").TargetPath -eq 'C:\Windows\notepad.exe') 'Installer changed an unrelated shortcut'
    Assert-True (@(Get-ChildItem $root -Force | Where-Object Name -Like '.civilized-*').Count -eq 0) 'Installation left staging or backup directories'
    Write-Output 'Bundle unit checks passed: compiled imports and licenses, file integrity, copying, repair and mocked host migration. Native startup and real host installation require test:bundle:installed.'
} finally {
    if ($null -eq $previousCivilizedAgentData) { Remove-Item Env:CIVILIZED_AGENT_DATA -ErrorAction SilentlyContinue }
    else { $env:CIVILIZED_AGENT_DATA = $previousCivilizedAgentData }
    Remove-DeploymentDirectory $temporary
}
