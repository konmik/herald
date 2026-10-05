param([string]$Destination = (Join-Path (Split-Path $PSScriptRoot -Parent) 'native-announcer/bin/gpu'))
$ErrorActionPreference = 'Stop'
$cudnn = @('cudnn64_9.dll', 'cudnn_adv64_9.dll', 'cudnn_cnn64_9.dll', 'cudnn_engines_precompiled64_9.dll', 'cudnn_engines_runtime_compiled64_9.dll', 'cudnn_graph64_9.dll', 'cudnn_heuristic64_9.dll', 'cudnn_ops64_9.dll')
$required = @('onnxruntime.dll', 'sherpa-onnx-c-api.dll', 'onnxruntime_providers_cuda.dll', 'onnxruntime_providers_shared.dll', 'cudart64_12.dll', 'cublas64_12.dll', 'cublasLt64_12.dll') + $cudnn
if (@($required | Where-Object { -not (Test-Path -LiteralPath (Join-Path $destination $_)) }).Count -eq 0) { return }
$stage = Join-Path (Split-Path $destination -Parent) ('gpu-install-' + [guid]::NewGuid())
$bundle = Join-Path $stage 'gpu'
New-Item -ItemType Directory -Path $bundle -Force | Out-Null
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
        Copy-Item -LiteralPath $file.FullName -Destination (Join-Path $bundle $name)
    }
    $cuda = Join-Path $stage 'cuda.zip'
    Get-Verified 'https://files.pythonhosted.org/packages/59/df/e7c3a360be4f7b93cee39271b792669baeb3846c58a4df6dfcf187a7ffab/nvidia_cuda_runtime_cu12-12.9.79-py3-none-win_amd64.whl' $cuda '8e018af8fa02363876860388bd10ccb89eb9ab8fb0aa749aaf58430a9f7c4891'
    Expand-Archive -LiteralPath $cuda -DestinationPath (Join-Path $stage 'cuda')
    Get-ChildItem (Join-Path $stage 'cuda') -Recurse -Filter cudart64_12.dll | Copy-Item -Destination (Join-Path $bundle 'cudart64_12.dll')
    $existing = 'C:/ComfyUI/venv/Lib/site-packages/torch/lib'
    if (@(@('cublas64_12.dll', 'cublasLt64_12.dll') + $cudnn | Where-Object { -not (Test-Path -LiteralPath (Join-Path $existing $_)) }).Count -eq 0) {
        foreach ($name in @('cublas64_12.dll', 'cublasLt64_12.dll')) {
            Copy-Item -LiteralPath (Join-Path $existing $name) -Destination (Join-Path $bundle $name)
        }
        foreach ($name in $cudnn) { Copy-Item -LiteralPath (Join-Path $existing $name) -Destination (Join-Path $bundle $name) }
    } else {
        $cublas = Join-Path $stage 'cublas.zip'
        Get-Verified 'https://files.pythonhosted.org/packages/20/e2/fc9a0e985249d873150276d5afb02e39a66817fedbf1a385724393e505ed/nvidia_cublas_cu12-12.9.2.10-py3-none-win_amd64.whl' $cublas '623f43027d40d44ceadf0043f002bd25cf353e8f13ce90b9a87057019f560661'
        Expand-Archive -LiteralPath $cublas -DestinationPath (Join-Path $stage 'cublas')
        Get-ChildItem (Join-Path $stage 'cublas') -Recurse -Filter '*.dll' | Copy-Item -Destination $bundle
        $cudnnArchive = Join-Path $stage 'cudnn.zip'
        Get-Verified 'https://files.pythonhosted.org/packages/aa/38/f856579877f7c1c5066e61182e7de7bc27bf35a78c8d1b0fa592e6985bc4/nvidia_cudnn_cu12-9.27.0.42-py3-none-win_amd64.whl' $cudnnArchive '06e9b0026f3bad97d2b58666330fabec04fe1672f776661ecb0ce0029c27f142'
        Expand-Archive -LiteralPath $cudnnArchive -DestinationPath (Join-Path $stage 'cudnn')
        Get-ChildItem (Join-Path $stage 'cudnn') -Recurse -Filter '*.dll' | Copy-Item -Destination $bundle
    }
    foreach ($name in $required) {
        if (-not (Test-Path -LiteralPath (Join-Path $bundle $name))) { throw "Missing GPU runtime asset: $name" }
    }
    & node (Join-Path $PSScriptRoot 'install-directory.mjs') $bundle $destination
    if ($LASTEXITCODE -ne 0) { throw 'GPU runtime installation failed' }
} finally {
    Remove-Item -LiteralPath $stage -Recurse -Force
}
