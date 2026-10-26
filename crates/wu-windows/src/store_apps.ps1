#Requires -Version 5.1
[CmdletBinding()]
param()
$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = New-Object System.Text.UTF8Encoding($false)

function Get-RemovableApps {
    [CmdletBinding()]
    param()
    Get-AppxPackage -PackageTypeFilter Main | Where-Object {
        # Fail closed when a property is absent. Frameworks, resources,
        # optional packages and protected Windows components are not apps.
        ($_.NonRemovable -eq $false) -and
        ($_.IsFramework -eq $false) -and
        ($_.IsResourcePackage -eq $false) -and
        ($_.IsPartiallyStaged -eq $false) -and
        ($_.SignatureKind -ne 'System') -and
        (-not ($_.InstallLocation -and $env:WATTCLEAN_SELF_EXE.StartsWith(
            $_.InstallLocation.TrimEnd('\') + '\', [StringComparison]::OrdinalIgnoreCase)))
    }
}

try {
    if ($env:WATTCLEAN_REMOVE_PACKAGE) {
        $package = @(Get-RemovableApps | Where-Object {
            $_.PackageFullName -ceq $env:WATTCLEAN_REMOVE_PACKAGE
        })
        if ($package.Count -ne 1) {
            throw 'This package is no longer installed or Windows protects it. Refresh the list.'
        }
        Remove-AppxPackage -Package $package[0].PackageFullName -ErrorAction Stop
        if (Get-AppxPackage -PackageTypeFilter Main | Where-Object {
            $_.PackageFullName -ceq $env:WATTCLEAN_REMOVE_PACKAGE
        }) {
            throw 'Windows still reports this package as installed.'
        }
    } else {
        $launchers = @(Get-StartApps)
        $apps = @(Get-RemovableApps | ForEach-Object {
            $package = $_
            $matched = @($launchers | Where-Object {
                $_.AppID.StartsWith($package.PackageFamilyName + '!', [StringComparison]::OrdinalIgnoreCase)
            })
            $name = $package.Name
            if ($matched.Count -eq 1) { $name = $matched[0].Name }
            [PSCustomObject]@{
                id = $package.PackageFullName
                name = $name
                publisher = $package.Publisher
                version = $package.Version.ToString()
            }
        } | Sort-Object name, id)
        ConvertTo-Json -InputObject $apps -Compress
    }
} catch {
    [Console]::Error.WriteLine($_.Exception.Message)
    exit 1
}
