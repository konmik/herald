param([string]$Output = "$PSScriptRoot\..\assets\recordings")
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Speech
Add-Type -ReferencedAssemblies System.Speech -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.Speech.Synthesis;
public class SpeechSample {
    public int viseme;
    public double time;
    public double duration;
}
public class SampleRecorder {
    public static SpeechSample[] Record(string text, string path) {
        var samples = new List<SpeechSample>();
        using (var speech = new SpeechSynthesizer()) {
            speech.SelectVoiceByHints(VoiceGender.Male, VoiceAge.Adult, 0, new System.Globalization.CultureInfo("en-US"));
            speech.Rate = 0;
            speech.VisemeReached += (sender, args) => samples.Add(new SpeechSample { viseme = args.Viseme, time = args.AudioPosition.TotalSeconds, duration = args.Duration.TotalSeconds });
            speech.SetOutputToWaveFile(path);
            speech.Speak(text);
        }
        return samples.ToArray();
    }
}
'@
New-Item -ItemType Directory -Force $Output | Out-Null
$sentences = [ordered]@{
    neutral = 'We made a plan. You can review it now.'
    pleased = 'The work is done. All checks have passed.'
    concerned = 'We found a problem. Please review the results.'
}
foreach ($emotion in $sentences.Keys) {
    $wav = Join-Path $Output "$emotion.wav"
    $samples = [SampleRecorder]::Record($sentences[$emotion], $wav)
    @{ text = $sentences[$emotion]; visemes = @($samples) } | ConvertTo-Json -Depth 5 | Set-Content (Join-Path $Output "$emotion.json") -Encoding utf8
    Write-Output $wav
}
