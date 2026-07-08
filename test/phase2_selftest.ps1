# Phase 2 self-test — licensing flow against the live Supabase backend
# (Windows PowerShell 5.1 compatible)
#
#  1. seed TEST-POS-1 license (unbound, active)      [PostgREST, service-role key]
#  2. activate-license HWID-A  -> ok:true + restaurant name
#  3. activate-license HWID-B  -> ok:false, bound-elsewhere
#  4. license-status  HWID-A   -> ok:true
#  5. license-status  HWID-B   -> ok:false, unbound
#  6. licenses.device_id       -> HWID-A              [PostgREST select]
#  7. cleanup: delete TEST-POS-1
#
# The service-role key is fetched at runtime from the Supabase CLI session
# (supabase CLI lives in ..\..\MB-backend as a dev dependency). No secrets in this file.

$ErrorActionPreference = 'Stop'
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
$ProjectRef = 'rlvygwituwywofwcwjsf'
$Base = "https://$ProjectRef.supabase.co"
$BackendDir = Join-Path (Split-Path -Parent (Split-Path -Parent $PSScriptRoot)) 'MB-backend'

Write-Host "Fetching service-role key via supabase CLI..."
Push-Location $BackendDir
try {
    $keysRaw = npx supabase projects api-keys --project-ref $ProjectRef 2>$null
} finally {
    Pop-Location
}
$keys = ($keysRaw | Out-String | ConvertFrom-Json).keys
$ServiceKey = ($keys | Where-Object { $_.name -eq 'service_role' }).api_key
if (-not $ServiceKey) { throw "Could not fetch service_role key. Run 'npx supabase login' in MB-backend first." }

$RestHeaders = @{
    'apikey'        = $ServiceKey
    'Authorization' = "Bearer $ServiceKey"
    'Prefer'        = 'return=representation,resolution=merge-duplicates'
}

$script:Results = @()
function Check([string]$Name, [bool]$Condition, $Actual) {
    if ($Condition) {
        Write-Host "  PASS  $Name" -ForegroundColor Green
        $script:Results += "| $Name | PASS |"
    } else {
        Write-Host "  FAIL  $Name" -ForegroundColor Red
        Write-Host "        actual: $($Actual | ConvertTo-Json -Compress -Depth 5)"
        $script:Results += "| $Name | **FAIL** |"
    }
}

function Invoke-Fn([string]$Name, $Body) {
    Invoke-RestMethod -Method Post -Uri "$Base/functions/v1/$Name" `
        -ContentType 'application/json' -Body ($Body | ConvertTo-Json -Depth 10)
}

# --- 1. Seed test license (upsert semantics: reset binding if row exists) ------
Write-Host "`n[1] Seed TEST-POS-1 license (unbound, active, +365 days)"
Invoke-RestMethod -Method Delete -Uri "$Base/rest/v1/licenses?key=eq.TEST-POS-1" -Headers $RestHeaders | Out-Null
$next = (Get-Date).ToUniversalTime().AddDays(365).ToString('o')
$seed = @{ key = 'TEST-POS-1'; restaurant_name = 'Test Cafe'; status = 'active'; next_billing_date = $next } | ConvertTo-Json
$lic = Invoke-RestMethod -Method Post -Uri "$Base/rest/v1/licenses" -Headers $RestHeaders -ContentType 'application/json' -Body $seed
Check "seed row created (unbound, active)" ($lic.key -eq 'TEST-POS-1' -and $lic.status -eq 'active' -and $null -eq $lic.device_id) $lic

# --- 2. Activate on device A ----------------------------------------------------
Write-Host "`n[2] activate-license HWID-A"
$r = Invoke-Fn 'activate-license' @{ key = 'TEST-POS-1'; deviceId = 'HWID-A'; deviceName = 'PC-A' }
Check "activate A -> ok:true"            ($r.ok -eq $true) $r
Check "restaurant name = Test Cafe"      ($r.user.restaurantName -eq 'Test Cafe') $r
Check "subscription status = active"     ($r.subscription.status -eq 'active') $r

# --- 3. Device B must be rejected -------------------------------------------------
Write-Host "`n[3] activate-license HWID-B (must be blocked)"
$r = Invoke-Fn 'activate-license' @{ key = 'TEST-POS-1'; deviceId = 'HWID-B'; deviceName = 'PC-B' }
Check "activate B -> ok:false"           ($r.ok -eq $false) $r
Check "reason = bound-elsewhere"         ($r.reason -eq 'bound-elsewhere') $r

# --- 4. Status from the bound device ----------------------------------------------
Write-Host "`n[4] license-status HWID-A"
$r = Invoke-Fn 'license-status' @{ key = 'TEST-POS-1'; deviceId = 'HWID-A' }
Check "status A -> ok:true"              ($r.ok -eq $true) $r

# --- 5. Status from the other device -> unbound ------------------------------------
Write-Host "`n[5] license-status HWID-B (must report unbound)"
$r = Invoke-Fn 'license-status' @{ key = 'TEST-POS-1'; deviceId = 'HWID-B' }
Check "status B -> ok:false"             ($r.ok -eq $false) $r
Check "reason = unbound"                 ($r.reason -eq 'unbound') $r

# --- 6. Verify the binding row directly ----------------------------------------------
Write-Host "`n[6] licenses.device_id via select"
$row = Invoke-RestMethod -Method Get -Headers $RestHeaders -Uri "$Base/rest/v1/licenses?key=eq.TEST-POS-1&select=device_id,device_name"
Check "device_id = HWID-A"               ($row[0].device_id -eq 'HWID-A') $row

# --- 7. Cleanup ------------------------------------------------------------------------
Write-Host "`n[7] Cleanup TEST-POS-1"
Invoke-RestMethod -Method Delete -Uri "$Base/rest/v1/licenses?key=eq.TEST-POS-1" -Headers $RestHeaders | Out-Null
$left = Invoke-RestMethod -Method Get -Headers $RestHeaders -Uri "$Base/rest/v1/licenses?key=eq.TEST-POS-1&select=key"
Check "test license removed"             (@($left).Count -eq 0) $left

# --- Result ------------------------------------------------------------------------------
$failed = ($script:Results | Where-Object { $_ -match 'FAIL' }).Count
Write-Host ""
Write-Output "RESULTS_TABLE_START"
$script:Results | ForEach-Object { Write-Output $_ }
Write-Output "RESULTS_TABLE_END"
if ($failed -eq 0) {
    Write-Host "PHASE2 SELF-TEST: ALL PASSED" -ForegroundColor Green
    exit 0
} else {
    Write-Host "PHASE2 SELF-TEST: $failed CHECK(S) FAILED" -ForegroundColor Red
    exit 1
}
