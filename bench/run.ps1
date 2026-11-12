# Head-to-head: Rust manim vs Python Manim CE, wall-clock, MP4 output, same scenes.
#
#   pwsh bench/run.ps1 -Manim <path to manim.exe> [-Runs 3] [-Qualities ql,qh]
#
# Both sides write H.264 MP4s through libx264. Times are full process wall-clock
# (startup + render + encode), median of -Runs. Frame counts are checked with ffprobe.

param(
    [Parameter(Mandatory = $true)][string]$Manim,
    [int]$Runs = 3,
    [string[]]$Qualities = @("ql", "qh"),
    [string[]]$Scenes = @("SquareToCircle", "Showcase", "Graphing", "Benchmark"),
    [string]$Out = "$env:TEMP\manim-bench",
    # Extra args for the Rust binaries, e.g. "--encoder","x264" for an encoder-matched run.
    [string[]]$RustArgs = @()
)
$ErrorActionPreference = "Stop"
$root = Split-Path $PSScriptRoot -Parent
Set-Location $root

$examples = @{ SquareToCircle = "square_to_circle"; Showcase = "showcase"; Graphing = "graphing"; Benchmark = "benchmark" }
cargo build -q --release --examples
if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }

$ffprobe = Get-ChildItem "$env:LOCALAPPDATA\Microsoft\WinGet\Packages" -Filter ffprobe.exe -Recurse -Depth 5 -ErrorAction SilentlyContinue | Select-Object -First 1 -ExpandProperty FullName
if (-not $ffprobe) { $ffprobe = "ffprobe" }

function Median($xs) { $s = $xs | Sort-Object; $s[[int][math]::Floor(($s.Count - 1) / 2)] }
function Frames($file) {
    & $ffprobe -v error -count_packets -select_streams v:0 -show_entries stream=nb_read_packets -of csv=p=0 $file
}
function Time($block) { (Measure-Command { & $block 2>&1 | Out-Null }).TotalSeconds }

$py = Join-Path (Split-Path $Manim) "python.exe"
$importTime = Median (1..$Runs | ForEach-Object { Time { & $py -c "import manim" } })
Write-Host ("Python `import manim` alone: {0:N2}s" -f $importTime)

$rows = @()
foreach ($q in $Qualities) {
    foreach ($scene in $Scenes) {
        $exe = ".\target\release\examples\$($examples[$scene]).exe"
        $rDir = "$Out\rust"; $pDir = "$Out\python"
        $rust = Median (1..$Runs | ForEach-Object { Time { & $exe "-$q" -q --media_dir $rDir @RustArgs } })
        $pyT = Median (1..$Runs | ForEach-Object { Time { & $Manim "-$q" --disable_caching --progress_bar none -v WARNING --media_dir $pDir bench\scenes.py $scene } })
        $rFile = Get-ChildItem "$rDir\videos\$scene" -Filter "$scene.mp4" -Recurse | Select-Object -First 1
        $pFile = Get-ChildItem "$pDir\videos\scenes" -Filter "$scene.mp4" -Recurse |
            Where-Object { $_.Directory.Name -like ("*p" + @{ ql = 15; qm = 30; qh = 60; qp = 60; qk = 60 }[$q]) -and $_.Directory.Name -like @{ ql = "480*"; qm = "720*"; qh = "1080*"; qp = "1440*"; qk = "2160*" }[$q] } |
            Select-Object -First 1
        $row = [pscustomobject]@{
            Quality      = $q
            Scene        = $scene
            "Rust (s)"   = [math]::Round($rust, 2)
            "Python (s)" = [math]::Round($pyT, 2)
            Speedup      = "{0:N1}x" -f ($pyT / $rust)
            "Frames R/P" = "$(Frames $rFile.FullName)/$(Frames $pFile.FullName)"
        }
        $row | Format-Table -HideTableHeaders | Out-String | Write-Host -NoNewline
        $rows += $row
    }
}
$rows | Format-Table -AutoSize
