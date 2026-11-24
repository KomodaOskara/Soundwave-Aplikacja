# Jednolinijkowy instalator SoundWave CLI dla Windows (x86_64)
# Użycie w PowerShell:
#   irm https://github.com/KomodaOskara/Soundwave-Aplikacja/releases/latest/download/install.ps1 | iex

$ErrorActionPreference = "Stop"

$Repo = if ($env:SWCLI_REPO) { $env:SWCLI_REPO } else { "KomodaOskara/Soundwave-Aplikacja" }
$BinUrl = $env:SWCLI_URL
$InstallDir = Join-Path $env:LOCALAPPDATA "soundwave-cli"

Write-Host "♪ Instalacja SoundWave CLI (Windows x86_64)..." -ForegroundColor Cyan

if (-not $BinUrl) {
    $releases = Invoke-RestMethod -Uri "https://api.github.com/repos/$Repo/releases?per_page=20" -Headers @{ "User-Agent" = "SoundWave-CLI-Installer" }
    foreach ($rel in $releases) {
        foreach ($asset in $rel.assets) {
            if ($asset.name -eq "soundwave-cli-windows-x86_64.exe") {
                $BinUrl = $asset.browser_download_url
                break
            }
        }
        if ($BinUrl) { break }
    }
}

if (-not $BinUrl) {
    Write-Error "Nie znaleziono pliku soundwave-cli-windows-x86_64.exe w wydaniach repo $Repo."
    exit 1
}

New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
$ExePath = Join-Path $InstallDir "soundwave-cli.exe"
$AliasPath = Join-Path $InstallDir "swcli.exe"

Write-Host "Pobieranie: $BinUrl"
Invoke-WebRequest -Uri $BinUrl -OutFile $ExePath
Copy-Item -Force $ExePath $AliasPath

# Dodaj katalog instalacyjny do PATH użytkownika (jeśli jeszcze go tam nie ma)
$UserPath = [Environment]::GetEnvironmentVariable("Path", "User")
if (-not $UserPath) { $UserPath = "" }
if ($UserPath -notlike "*$InstallDir*") {
    $NewPath = if ($UserPath.Trim().EndsWith(";") -or $UserPath -eq "") { "$UserPath$InstallDir" } else { "$UserPath;$InstallDir" }
    [Environment]::SetEnvironmentVariable("Path", $NewPath, "User")
    $env:Path = "$env:Path;$InstallDir"
    Write-Host "✓ Dodano $InstallDir do zmiennej PATH użytkownika" -ForegroundColor Green
}

# Sprawdź ffmpeg (wymagane do dekodowania strumienia audio)
if (-not (Get-Command ffmpeg -ErrorAction SilentlyContinue)) {
    Write-Host ""
    Write-Host "⚠ Nie wykryto ffmpeg w PATH (potrzebne do odtwarzania audio)." -ForegroundColor Yellow
    if (Get-Command winget -ErrorAction SilentlyContinue) {
        Write-Host "Zainstaluj poleceniem:  winget install Gyan.FFmpeg" -ForegroundColor Yellow
    } else {
        Write-Host "Pobierz ffmpeg z https://www.gyan.dev/ffmpeg/builds/ i dodaj do PATH." -ForegroundColor Yellow
    }
}

Write-Host ""
Write-Host "✓ Gotowe! Otwórz nowy terminal (Windows Terminal / PowerShell) i wpisz:" -ForegroundColor Green
Write-Host "   swcli   (albo soundwave-cli)" -ForegroundColor White
