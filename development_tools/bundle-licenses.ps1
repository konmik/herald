function Copy-BundledLicenses {
    param([string]$Metafile, [string]$Root, [string]$Destination)
    $metadata = Get-Content -LiteralPath $Metafile -Raw | ConvertFrom-Json
    $packages = [Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
    foreach ($inputFile in $metadata.inputs.PSObject.Properties.Name) {
        if ($inputFile.Replace('\', '/') -notmatch '^(.*(?:^|/)node_modules/(?:@[^/]+/)?[^/]+)(?:/|$)') { continue }
        $directory = [IO.Path]::GetFullPath($Matches[1], $Root)
        if (-not $packages.Add($directory)) { continue }
        $package = Get-Content -LiteralPath (Join-Path $directory 'package.json') -Raw | ConvertFrom-Json
        $licenses = @(Get-ChildItem -LiteralPath $directory -File | Where-Object Name -Match '^(LICENSE|LICENCE|COPYING|NOTICE)(\b|[-._])')
        if (-not $licenses.Count -and $package.name.StartsWith('@opencode/')) {
            $pinned = Join-Path $PSScriptRoot "licenses/opencode-$($package.version)-LICENSE"
            if (Test-Path -LiteralPath $pinned) { $licenses = @(Get-Item -LiteralPath $pinned) }
        }
        if (-not $licenses.Count) { throw "Bundled package has no license file: $($package.name)@$($package.version)" }
        $target = Join-Path $Destination ($package.name.Replace('/', '-') + '-' + $package.version)
        [IO.Directory]::CreateDirectory($target) | Out-Null
        foreach ($file in $licenses) { Copy-Item -LiteralPath $file.FullName -Destination (Join-Path $target $file.Name) -Force }
    }
}
