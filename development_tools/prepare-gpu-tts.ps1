$ErrorActionPreference = 'Stop'
$repository = Split-Path $PSScriptRoot -Parent
$destination = Join-Path $repository 'native-announcer/bin/gpu'
$required = @('onnxruntime.dll', 'sherpa-onnx-c-api.dll', 'onnxruntime_providers_cuda.dll', 'onnxruntime_providers_shared.dll', 'cudart64_12.dll', 'cublas64_12.dll', 'cublasLt64_12.dll')
if (@($required | Where-Object { -not (Test-Path -LiteralPath (Join-Path $destination $_)) }).Count -eq 0) { return }
$stage = Join-Path $env:LOCALAPPDATA ('Temp/opencode/kitten-gpu-' + [guid]::NewGuid())
New-Item -ItemType Directory -Path $stage, $destination -Force | Out-Null
function Get-Verified {
    param([string]$Url, [string]$File, [string]$Hash)
    Invoke-WebRequest -Uri $Url -OutFile $File
    if ((Get-FileHash $File -Algorithm SHA256).Hash.ToLowerInvariant() -ne $Hash) { throw 'GPU runtime checksum mismatch' }
}
try {
    $archive = Join-Path $stage 'runtime.tar.bz2'
    Get-Verified 'https://github.com/k2-fsa/sherpa-onnx/releases/download/v1.13.8/sherpa-onnx-v1.13.8-cuda-12.x-cudnn-9.x-onnxruntime1.28.2-win-x64-cuda.tar.bz2' $archive '066c5b54dbafaa1388001a9c9837ac1374dbba6d6678f193ca06aa0d8e94d8c3'
    & tar -xjf $archive -C $stage
    if ($LASTEXITCODE -ne 0) { throw 'GPU runtime extraction failed' }
    foreach ($name in @('onnxruntime.dll', 'sherpa-onnx-c-api.dll', 'onnxruntime_providers_cuda.dll', 'onnxruntime_providers_shared.dll')) {
        $file = Get-ChildItem $stage -Recurse -Filter $name | Select-Object -First 1
        Copy-Item -LiteralPath $file.FullName -Destination (Join-Path $destination $name)
    }
    $cuda = Join-Path $stage 'cuda.zip'
    Get-Verified 'https://files.pythonhosted.org/packages/59/df/e7c3a360be4f7b93cee39271b792669baeb3846c58a4df6dfcf187a7ffab/nvidia_cuda_runtime_cu12-12.9.79-py3-none-win_amd64.whl' $cuda '8e018af8fa02363876860388bd10ccb89eb9ab8fb0aa749aaf58430a9f7c4891'
    Expand-Archive -LiteralPath $cuda -DestinationPath (Join-Path $stage 'cuda')
    Get-ChildItem (Join-Path $stage 'cuda') -Recurse -Filter cudart64_12.dll | Copy-Item -Destination (Join-Path $destination 'cudart64_12.dll')
    $existing = 'C:/ComfyUI/venv/Lib/site-packages/torch/lib'
    if (Test-Path (Join-Path $existing 'cublasLt64_12.dll')) {
        foreach ($name in @('cublas64_12.dll', 'cublasLt64_12.dll')) {
            New-Item -ItemType HardLink -Path (Join-Path $destination $name) -Target (Join-Path $existing $name) | Out-Null
        }
        Get-ChildItem $existing -Filter 'cudnn*.dll' | ForEach-Object { New-Item -ItemType HardLink -Path (Join-Path $destination $_.Name) -Target $_.FullName | Out-Null }
    } else {
        $cublas = Join-Path $stage 'cublas.zip'
        Get-Verified 'https://files.pythonhosted.org/packages/20/e2/fc9a0e985249d873150276d5afb02e39a66817fedbf1a385724393e505ed/nvidia_cublas_cu12-12.9.2.10-py3-none-win_amd64.whl' $cublas '623f43027d40d44ceadf0043f002bd25cf353e8f13ce90b9a87057019f560661'
        Expand-Archive -LiteralPath $cublas -DestinationPath (Join-Path $stage 'cublas')
        Get-ChildItem (Join-Path $stage 'cublas') -Recurse -Filter '*.dll' | Copy-Item -Destination $destination
    }
} finally {
    Remove-Item -LiteralPath $stage -Recurse -Force
}
