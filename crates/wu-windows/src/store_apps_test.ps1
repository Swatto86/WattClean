#Requires -Version 5.1
# Test-only cmdlet stand-ins. No call can reach Appx deployment.
[CmdletBinding()]
param()
$ErrorActionPreference = 'Stop'
$script:removed = $false

function Get-AppxPackage {
    [CmdletBinding()]
    param([string] $PackageTypeFilter)
    if ($PackageTypeFilter -ne 'Main') { throw 'Expected Main packages only.' }
    foreach ($name in @('Notes', 'Protected', 'Framework', 'Blocked', 'Sticky')) {
        if ($name -eq 'Notes' -and $script:removed) { continue }
        [PSCustomObject]@{
            Name = 'Contoso.' + $name
            PackageFullName = 'Contoso.' + $name + '_1.0.0.0_x64__abcdefghijklm'
            PackageFamilyName = 'Contoso.' + $name + '_abcdefghijklm'
            Version = [version]'1.0.0.0'
            Publisher = 'Contoso'
            NonRemovable = ($name -eq 'Protected')
            IsFramework = ($name -eq 'Framework')
            IsResourcePackage = $false
            IsPartiallyStaged = $false
            SignatureKind = 'Store'
            InstallLocation = 'C:\Program Files\WindowsApps\Contoso.' + $name
        }
    }
}

function Get-StartApps {
    [CmdletBinding()]
    param()
    [PSCustomObject]@{ Name = 'Contoso caf' + [char]0xe9; AppID = 'Contoso.Notes_abcdefghijklm!App' }
}

function Remove-AppxPackage {
    [CmdletBinding()]
    param([string] $Package)
    switch -Exact ($Package) {
        'Contoso.Notes_1.0.0.0_x64__abcdefghijklm' { $script:removed = $true }
        'Contoso.Sticky_1.0.0.0_x64__abcdefghijklm' { }
        default { throw 'Removal refused by test deployment service.' }
    }
}
