# Phase 3 self-test — bill sync flow against the live Supabase backend
# (Windows PowerShell 5.1 compatible)
#
#  1. seed TEST-SYNC-1 license pre-bound to HWID-SYNC   [PostgREST, service-role key]
#  2. sync-bills with 2 fake bills            -> ok:true, saved:2
#  3. re-send SAME 2 bills (idempotent upsert) -> ok:true, saved:2; count stays 2
#  4. sync-bills with wrong deviceId           -> ok:false, reason unbound
#  5. items jsonb round-trip verified via select
#  6. cleanup (license delete cascades bills; both verified)
#
# Service-role key is fetched at runtime from the Supabase CLI session in
# ..\..\MB-backend. No secrets stored in this file.

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
    'Prefer'        = 'return=representation'
}

$script:Results = @()
function Check([string]$Name, [bool]$Condition, $Actual) {
    if ($Condition) {
        Write-Host "  PASS  $Name" -ForegroundColor Green
        $script:Results += "| $Name | PASS |"
    } else {
        Write-Host "  FAIL  $Name" -ForegroundColor Red
        Write-Host "        actual: $($Actual | ConvertTo-Json -Compress -Depth 6)"
        $script:Results += "| $Name | **FAIL** |"
    }
}

function Invoke-Fn([string]$Name, $Body) {
    # Like the app's fetch(): non-2xx statuses still return the JSON body
    # (sync-bills answers 403 for unbound/invalid-key).
    try {
        Invoke-RestMethod -Method Post -Uri "$Base/functions/v1/$Name" `
            -ContentType 'application/json' -Body ($Body | ConvertTo-Json -Depth 10)
    } catch [System.Net.WebException] {
        $resp = $_.Exception.Response
        if ($null -eq $resp) { throw }
        $sr = New-Object IO.StreamReader($resp.GetResponseStream())
        $sr.ReadToEnd() | ConvertFrom-Json
    }
}

# --- 1. Seed pre-bound license --------------------------------------------------
Write-Host "`n[1] Seed TEST-SYNC-1 license (bound to HWID-SYNC)"
Invoke-RestMethod -Method Delete -Uri "$Base/rest/v1/licenses?key=eq.TEST-SYNC-1" -Headers $RestHeaders | Out-Null
$next = (Get-Date).ToUniversalTime().AddDays(365).ToString('o')
$seed = @{ key = 'TEST-SYNC-1'; restaurant_name = 'Sync Cafe'; status = 'active'; next_billing_date = $next; device_id = 'HWID-SYNC'; device_name = 'Sync Test PC' } | ConvertTo-Json
$lic = Invoke-RestMethod -Method Post -Uri "$Base/rest/v1/licenses" -Headers $RestHeaders -ContentType 'application/json' -Body $seed
Check "license seeded, bound to HWID-SYNC" ($lic.key -eq 'TEST-SYNC-1' -and $lic.device_id -eq 'HWID-SYNC') $lic

# --- 2. Push 2 fake bills ----------------------------------------------------------
Write-Host "`n[2] sync-bills with 2 fake bills"
$now = (Get-Date).ToUniversalTime().ToString('o')
$bills = @(
    @{ local_id = 9001; bill_number = 'P3-1'; token_number = 101; order_type = 'Table'; table_number = '4'
       customer_name = ''; customer_phone = ''; payment_mode = 'Cash'; subtotal = 150; gst = 7.5; total = 157.5
       items = @(@{ name = 'Idli'; qty = 2; price = 40 }, @{ name = 'Vada'; qty = 1; price = 70 }); billed_at = $now },
    @{ local_id = 9002; bill_number = 'P3-2'; token_number = 102; order_type = 'Parcel'; table_number = ''
       customer_name = 'Ravi'; customer_phone = '9999999999'; payment_mode = 'UPI'; subtotal = 220; gst = 11; total = 231
       items = @(@{ name = 'Masala Dosa'; qty = 2; price = 110 }); billed_at = $now }
)
$r = Invoke-Fn 'sync-bills' @{ key = 'TEST-SYNC-1'; deviceId = 'HWID-SYNC'; bills = $bills }
Check "first push ok:true"  ($r.ok -eq $true) $r
Check "first push saved:2"  ($r.saved -eq 2) $r

# --- 3. Idempotent re-send ------------------------------------------------------------
Write-Host "`n[3] Re-send the SAME 2 bills (upsert, no duplicates)"
$r = Invoke-Fn 'sync-bills' @{ key = 'TEST-SYNC-1'; deviceId = 'HWID-SYNC'; bills = $bills }
Check "re-send ok:true"  ($r.ok -eq $true) $r
Check "re-send saved:2"  ($r.saved -eq 2) $r
$count = Invoke-RestMethod -Method Get -Headers $RestHeaders -Uri "$Base/rest/v1/bills?license_key=eq.TEST-SYNC-1&select=local_id"
Check "cloud row count is exactly 2" (@($count).Count -eq 2) $count

# --- 4. Wrong device rejected -----------------------------------------------------------
Write-Host "`n[4] sync-bills with wrong deviceId"
$r = Invoke-Fn 'sync-bills' @{ key = 'TEST-SYNC-1'; deviceId = 'HWID-WRONG'; bills = @($bills[0]) }
Check "wrong device ok:false"    ($r.ok -eq $false) $r
Check "reason = unbound"         ($r.reason -eq 'unbound') $r

# --- 5. items jsonb round-trip -------------------------------------------------------------
Write-Host "`n[5] Verify items jsonb round-trip"
$stored = Invoke-RestMethod -Method Get -Headers $RestHeaders `
    -Uri "$Base/rest/v1/bills?license_key=eq.TEST-SYNC-1&local_id=eq.9001&select=local_id,items,total"
$item0 = $stored[0].items[0]
Check "items present with name+qty" ($null -ne $item0 -and $item0.name -eq 'Idli' -and $item0.qty -eq 2) $stored

# --- 6. Cleanup -------------------------------------------------------------------------------
Write-Host "`n[6] Cleanup"
Invoke-RestMethod -Method Delete -Uri "$Base/rest/v1/licenses?key=eq.TEST-SYNC-1" -Headers $RestHeaders | Out-Null
$leftB = Invoke-RestMethod -Method Get -Headers $RestHeaders -Uri "$Base/rest/v1/bills?license_key=eq.TEST-SYNC-1&select=local_id"
$leftL = Invoke-RestMethod -Method Get -Headers $RestHeaders -Uri "$Base/rest/v1/licenses?key=eq.TEST-SYNC-1&select=key"
Check "license removed"       (@($leftL).Count -eq 0) $leftL
Check "bills cascade-removed" (@($leftB).Count -eq 0) $leftB

# --- Result --------------------------------------------------------------------------------------
$failed = ($script:Results | Where-Object { $_ -match 'FAIL' }).Count
Write-Host ""
Write-Output "RESULTS_TABLE_START"
$script:Results | ForEach-Object { Write-Output $_ }
Write-Output "RESULTS_TABLE_END"
if ($failed -eq 0) {
    Write-Host "PHASE3 SELF-TEST: ALL PASSED" -ForegroundColor Green
    exit 0
} else {
    Write-Host "PHASE3 SELF-TEST: $failed CHECK(S) FAILED" -ForegroundColor Red
    exit 1
}
