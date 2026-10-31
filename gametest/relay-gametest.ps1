<#
Relay game test runner for Windows. Prepares its own Minecraft + Fabric copy (shared by every run),
then starts the game hidden with the Relay test bot and a scenario.

  prepare : download/verify game files, print JSON status
  start   : prepare + launch in background, print JSON with pid and out dir
  status  : JSON with running/finished, report, files

Nothing here touches other launchers: their asset folders are only read to avoid re-downloading.
#>
param(
    [ValidateSet('prepare', 'start', 'status')] [string]$Action = 'start',
    [string]$Root = (Join-Path $env:LOCALAPPDATA 'Relay\gametest'),
    [string]$Version = '1.21.8',
    [string]$Loader = '',
    [string]$Mods = '',          # folder with jars to test (mod under test, fabric-api, testbot)
    [string]$Scenario = '',
    [string]$Run = '',           # run id; output goes to $Root\runs\<id>
    [int]$Width = 1280,
    [int]$Height = 720,
    [int]$MemoryMb = 3072,
    [switch]$Visible
)
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
Add-Type -AssemblyName System.Net.Http
$http = [System.Net.Http.HttpClient]::new()
$http.Timeout = [TimeSpan]::FromMinutes(5)

function Out-Json($o) { $o | ConvertTo-Json -Depth 6 -Compress; exit 0 }
function Get-Json([string]$url) { $http.GetStringAsync($url).GetAwaiter().GetResult() | ConvertFrom-Json }
function Get-Sha1([string]$path) { (Get-FileHash -Algorithm SHA1 $path).Hash.ToLower() }

# Downloads many files in parallel; each item: @{ url; path; sha1 }.
function Save-Files($items) {
    $todo = @($items | Where-Object { -not (Test-Path $_.path) -or ($_.sha1 -and (Get-Sha1 $_.path) -ne $_.sha1) })
    for ($i = 0; $i -lt $todo.Count; $i += 24) {
        $batch = $todo[$i..([Math]::Min($i + 23, $todo.Count - 1))]
        $tasks = foreach ($f in $batch) {
            New-Item -ItemType Directory -Force (Split-Path $f.path) | Out-Null
            @{ f = $f; t = $http.GetByteArrayAsync($f.url) }
        }
        foreach ($x in $tasks) {
            $bytes = $x.t.GetAwaiter().GetResult()
            [IO.File]::WriteAllBytes($x.f.path + '.part', $bytes)
            Move-Item -Force ($x.f.path + '.part') $x.f.path
        }
    }
    $todo.Count
}

function Test-Rules($rules) {
    if (-not $rules) { return $true }
    $allowed = $false
    foreach ($r in $rules) {
        $match = (-not $r.os) -or ($r.os.name -eq 'windows' -and -not $r.os.arch)
        if ($r.features) { $match = $false }
        if ($match) { $allowed = $r.action -eq 'allow' }
    }
    $allowed
}

function Find-Java {
    $candidates = @()
    if ($env:JAVA_HOME) { $candidates += Join-Path $env:JAVA_HOME 'bin\javaw.exe' }
    $candidates += Get-Command javaw -All -ErrorAction SilentlyContinue | ForEach-Object Source
    foreach ($dir in @("$env:ProgramFiles\Eclipse Adoptium", "$env:ProgramFiles\Java", "$env:ProgramFiles\Microsoft",
                       "$env:ProgramFiles\Zulu", "$env:APPDATA", "$env:LOCALAPPDATA\Packages\Microsoft.4297127D64EC6_8wekyb3d8bbwe\LocalCache\Local\runtime")) {
        if (Test-Path $dir) {
            $candidates += Get-ChildItem $dir -Recurse -Filter javaw.exe -Depth 6 -ErrorAction SilentlyContinue | ForEach-Object FullName
        }
    }
    foreach ($java in $candidates | Select-Object -Unique) {
        $release = Join-Path (Split-Path (Split-Path $java)) 'release'
        if ((Test-Path $release) -and ((Get-Content $release -Raw) -match 'JAVA_VERSION="(\d+)')) {
            if ([int]$Matches[1] -ge 21) { return $java }
        }
    }
    $null
}

function Find-AssetSources {
    # Any launcher's assets/objects folder can seed ours; files are checked by hash anyway.
    $dirs = @("$env:APPDATA\.minecraft\assets")
    foreach ($base in @($env:APPDATA, $env:LOCALAPPDATA)) {
        $dirs += Get-ChildItem $base -Directory -ErrorAction SilentlyContinue |
            ForEach-Object { Get-ChildItem $_.FullName -Directory -Filter assets -Recurse -Depth 3 -ErrorAction SilentlyContinue } |
            ForEach-Object FullName
    }
    $dirs | Where-Object { Test-Path (Join-Path $_ 'objects') } | Select-Object -Unique
}

function Invoke-Prepare {
    $game = Join-Path $Root 'game'
    $versionDir = Join-Path $game "versions\$Version"
    New-Item -ItemType Directory -Force $versionDir | Out-Null
    $manifest = Get-Json 'https://piston-meta.mojang.com/mc/game/version_manifest_v2.json'
    $entry = $manifest.versions | Where-Object id -eq $Version
    if (-not $entry) { throw "Нет версии Minecraft $Version" }
    $vjPath = Join-Path $versionDir "$Version.json"
    if (-not (Test-Path $vjPath) -or (Get-Sha1 $vjPath) -ne $entry.sha1) {
        [IO.File]::WriteAllText($vjPath, $http.GetStringAsync($entry.url).GetAwaiter().GetResult())
    }
    $vj = Get-Content $vjPath -Raw | ConvertFrom-Json

    if (-not $Loader) {
        # PowerShell 5.1 passes a parsed JSON array down the pipeline as one object: unroll it first.
        $loaders = @(Get-Json "https://meta.fabricmc.net/v2/versions/loader/$Version") | ForEach-Object { $_ }
        $Loader = @($loaders | Where-Object { $_.loader.stable })[0].loader.version
    }
    $fjPath = Join-Path $versionDir "fabric-$Loader.json"
    if (-not (Test-Path $fjPath)) {
        [IO.File]::WriteAllText($fjPath, $http.GetStringAsync("https://meta.fabricmc.net/v2/versions/loader/$Version/$Loader/profile/json").GetAwaiter().GetResult())
    }
    $fj = Get-Content $fjPath -Raw | ConvertFrom-Json

    $files = @(@{ url = $vj.downloads.client.url; path = Join-Path $versionDir "$Version.jar"; sha1 = $vj.downloads.client.sha1 })
    $classpath = @()
    $natives = @()
    foreach ($lib in $fj.libraries) {
        $g, $a, $v = $lib.name.Split(':')
        $rel = "$($g.Replace('.', '/'))/$a/$v/$a-$v.jar"
        $path = Join-Path $game "libraries\$rel"
        $files += @{ url = $lib.url.TrimEnd('/') + "/$rel"; path = $path; sha1 = $lib.sha1 }
        $classpath += $path
    }
    $fabricNames = $fj.libraries | ForEach-Object { ($_.name.Split(':'))[0..1] -join ':' }
    foreach ($lib in $vj.libraries) {
        if (-not (Test-Rules $lib.rules) -or -not $lib.downloads.artifact) { continue }
        $parts = $lib.name.Split(':')
        if ($fabricNames -contains ($parts[0..1] -join ':') -and $parts.Count -eq 3) { continue }
        # Only this machine's natives: arm64/x86 jars carry DLLs with the same names.
        $arch = if ($env:PROCESSOR_ARCHITECTURE -eq 'ARM64') { 'natives-windows-arm64' } else { 'natives-windows' }
        if ($parts.Count -gt 3 -and $parts[3] -like 'natives-windows*' -and $parts[3] -ne $arch) { continue }
        $art = $lib.downloads.artifact
        $path = Join-Path $game "libraries\$($art.path)"
        $files += @{ url = $art.url; path = $path; sha1 = $art.sha1 }
        $classpath += $path
        if ($parts.Count -gt 3 -and $parts[3] -like 'natives-*') { $natives += $path }
    }
    $fetched = Save-Files $files

    # Natives: LWJGL can load from the jars, but an extracted folder is the most predictable.
    $nativeDir = Join-Path $versionDir 'natives'
    if (-not (Test-Path $nativeDir)) {
        New-Item -ItemType Directory -Force $nativeDir | Out-Null
        Add-Type -AssemblyName System.IO.Compression.FileSystem
        foreach ($jar in $natives) {
            $zip = [IO.Compression.ZipFile]::OpenRead($jar)
            foreach ($e in $zip.Entries | Where-Object { $_.Name -like '*.dll' }) {
                [IO.Compression.ZipFileExtensions]::ExtractToFile($e, (Join-Path $nativeDir $e.Name), $true)
            }
            $zip.Dispose()
        }
    }

    $assets = Join-Path $game 'assets'
    $indexPath = Join-Path $assets "indexes\$($vj.assetIndex.id).json"
    Save-Files @(@{ url = $vj.assetIndex.url; path = $indexPath; sha1 = $vj.assetIndex.sha1 }) | Out-Null
    $index = Get-Content $indexPath -Raw | ConvertFrom-Json
    $sources = @(Find-AssetSources | Where-Object { $_ -ne $assets })
    $objects = @()
    $copied = 0
    foreach ($p in $index.objects.PSObject.Properties) {
        $h = $p.Value.hash
        $rel = "$($h.Substring(0, 2))\$h"
        $target = Join-Path $assets "objects\$rel"
        if (-not (Test-Path $target)) {
            $seed = $sources | ForEach-Object { Join-Path $_ "objects\$rel" } | Where-Object { Test-Path $_ } | Select-Object -First 1
            if ($seed) {
                New-Item -ItemType Directory -Force (Split-Path $target) | Out-Null
                Copy-Item $seed $target
                $copied++
                continue
            }
            $objects += @{ url = "https://resources.download.minecraft.net/$($h.Substring(0, 2))/$h"; path = $target; sha1 = $null }
        }
    }
    $fetched += Save-Files $objects

    $java = Find-Java
    [IO.File]::WriteAllText((Join-Path $versionDir 'launch.json'), (@{
        java = $java; main = $fj.mainClass; classpath = ($classpath + (Join-Path $versionDir "$Version.jar")) -join ';'
        natives = $nativeDir; assets = $assets; assetIndex = $vj.assetIndex.id; loader = $Loader
    } | ConvertTo-Json))
    @{ ok = [bool]$java; version = $Version; loader = $Loader; java = $java; downloaded = $fetched; copiedAssets = $copied; root = $Root }
}

if ($Action -eq 'prepare') { Out-Json (Invoke-Prepare) }

if ($Action -eq 'start') {
    if (-not $Scenario -or -not (Test-Path $Scenario)) { throw 'Нужен -Scenario' }
    $prep = Invoke-Prepare
    if (-not $prep.java) { Out-Json @{ ok = $false; error = 'Не найдена Java 21+. Установи Temurin 21 или подключи лаунчер Minecraft.' } }
    $launch = Get-Content (Join-Path $Root "game\versions\$Version\launch.json") -Raw | ConvertFrom-Json
    if (-not $Run) { $Run = Get-Date -Format 'yyyyMMdd-HHmmss' }
    $out = Join-Path $Root "runs\$Run"
    $instance = Join-Path $Root 'instance'
    $modsDir = Join-Path $instance 'mods'
    New-Item -ItemType Directory -Force $out, $modsDir | Out-Null
    Get-ChildItem $modsDir -Filter *.jar | Remove-Item -Force
    if ($Mods) { Get-ChildItem $Mods -Filter *.jar | Copy-Item -Destination $modsDir }
    Copy-Item $Scenario (Join-Path $out 'scenario.txt')
    $jvm = @("-Xmx$($MemoryMb)m", "-Djava.library.path=$($launch.natives)",
        "-Drelay.test.scenario=$(Join-Path $out 'scenario.txt')", "-Drelay.test.out=$out")
    if ($Visible) { $jvm += '-Drelay.test.hidden=false' }
    $jvm += @('-cp', $launch.classpath, $launch.main,
        '--username', 'RelayTest', '--uuid', '5a1d7e57-0000-3000-8000-000000000001', '--accessToken', '0', '--userType', 'legacy',
        '--version', $Version, '--versionType', 'release', '--gameDir', $instance,
        '--assetsDir', $launch.assets, '--assetIndex', $launch.assetIndex,
        '--width', $Width, '--height', $Height)
    $quoted = $jvm | ForEach-Object { if ($_ -match '[\s"]') { '"' + ($_ -replace '"', '\"') + '"' } else { $_ } }
    $p = Start-Process -FilePath $launch.java -ArgumentList $quoted -WorkingDirectory $instance -PassThru `
        -RedirectStandardOutput (Join-Path $out 'stdout.log') -RedirectStandardError (Join-Path $out 'stderr.log')
    Set-Content (Join-Path $out 'pid') $p.Id
    Out-Json @{ ok = $true; pid = $p.Id; run = $Run; out = $out; java = $launch.java; loader = $launch.loader; downloaded = $prep.downloaded }
}

if ($Action -eq 'status') {
    $out = Join-Path $Root "runs\$Run"
    $procId = Get-Content (Join-Path $out 'pid') -ErrorAction SilentlyContinue
    $running = [bool]($procId -and (Get-Process -Id $procId -ErrorAction SilentlyContinue))
    $report = Join-Path $out 'report.json'
    Out-Json @{
        running = $running; out = $out
        report = if (Test-Path $report) { Get-Content $report -Raw | ConvertFrom-Json } else { $null }
        files = @(Get-ChildItem $out -File | ForEach-Object { @{ name = $_.Name; size = $_.Length } })
        tail = @(Get-Content (Join-Path $out 'stdout.log') -Tail 15 -ErrorAction SilentlyContinue | ForEach-Object { [string]$_ })
    }
}
