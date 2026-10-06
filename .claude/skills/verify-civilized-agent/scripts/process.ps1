function Test-OwnedPlaybackProcess {
    param($Process, [string]$Binary, [string]$ErrorLog)
    $Process.Refresh()
    if ($Process.HasExited) { return $false }
    try { $path = $Process.Path } catch {
        if ($Process.HasExited) { return $false }
        throw
    }
    if ($Process.HasExited) { return $false }
    if ($path -ne $Binary -or (Test-Path -LiteralPath $ErrorLog)) {
        throw 'Doctor failed: wrong process or runtime errors'
    }
    return $true
}
