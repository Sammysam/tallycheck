<#
.SYNOPSIS
  Opens TallyCheck's port to the office network only (run as Administrator on the main PC).

.DESCRIPTION
  Adds one inbound Windows Firewall rule for the app's TCP port that applies to the Private
  network profile and to the local subnet only. Public networks (cafés, hotel Wi-Fi) and
  addresses outside the subnet stay blocked by Windows, in addition to the app's own
  allowed-network check (lan_allowed_networks in config.toml).

  -Remove deletes the rule again.

.EXAMPLE
  powershell -ExecutionPolicy Bypass -File scripts\lan-firewall.ps1
  powershell -ExecutionPolicy Bypass -File scripts\lan-firewall.ps1 -Port 47800 -Remove
#>
param(
  [int]$Port = 47800,
  [switch]$Remove
)

$ErrorActionPreference = 'Stop'
$name = "TallyCheck (office LAN, TCP $Port)"

$admin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole(
  [Security.Principal.WindowsBuiltInRole]::Administrator)
if (-not $admin) { throw 'Run this script from an elevated PowerShell (Run as administrator).' }

Get-NetFirewallRule -DisplayName $name -ErrorAction SilentlyContinue | Remove-NetFirewallRule
if ($Remove) { Write-Host "Removed firewall rule: $name"; return }

New-NetFirewallRule -DisplayName $name -Direction Inbound -Action Allow -Protocol TCP -LocalPort $Port `
  -Profile Private -RemoteAddress LocalSubnet | Out-Null
Write-Host "Added firewall rule: $name (Private network profile, local subnet only)."

$profiles = Get-NetConnectionProfile | Select-Object InterfaceAlias, NetworkCategory
$profiles | Format-Table -AutoSize
if ($profiles | Where-Object { $_.NetworkCategory -ne 'Private' -and $_.NetworkCategory -ne 'DomainAuthenticated' }) {
  Write-Warning 'An active network is not marked Private; the rule does not apply to it. Mark the office network as Private in Windows Settings > Network.'
}
