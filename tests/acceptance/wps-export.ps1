param(
    [string]$Docx,
    [string]$Pdf,
    [string]$Only
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$exportRoot = Join-Path $root 'output\render-trials'
$status = [ordered]@{}
$statusPath = Join-Path $exportRoot 'status-wps.json'
if (Test-Path $statusPath) {
    $previous = Get-Content $statusPath -Raw | ConvertFrom-Json
    foreach ($property in $previous.PSObject.Properties) {
        $status[$property.Name] = $property.Value
    }
}

$app = New-Object -ComObject Kwps.Application
try {
    $app.Visible = $false
    $app.DisplayAlerts = 0
    function Export-Document($inputPath, $outputPath) {
        $document = $app.Documents.Open([IO.Path]::GetFullPath($inputPath), $false, $true, $false)
        try {
            $outputPath = [IO.Path]::GetFullPath($outputPath)
            if (Test-Path -LiteralPath $outputPath) { Remove-Item -LiteralPath $outputPath }
            try {
                $document.ExportAsFixedFormat($outputPath, 17, $false, 0, 0)
                $method = 'ExportAsFixedFormat(17)'
            } catch {
                $document.SaveAs($outputPath, 17)
                $method = 'SaveAs(17)'
            }
            if (!(Test-Path $outputPath)) { throw "WPS did not write $outputPath" }
            return $method
        } finally {
            $document.Close(0)
            [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($document)
        }
    }
    if ($Docx) {
        Export-Document $Docx $Pdf
    } else {
        New-Item -ItemType Directory -Path $exportRoot -Force | Out-Null
        Get-ChildItem $exportRoot -Recurse -Filter tydocx.docx | ForEach-Object {
            $name = $_.Directory.Name
            if (!$Only -or $Only -eq $name) {
                try {
                    $method = Export-Document $_.FullName (Join-Path $_.DirectoryName 'wps.pdf')
                    $status[$name] = @{ wrote = $true; method = $method }
                    Write-Host "$name $method"
                } catch {
                    $status[$name] = @{ wrote = $false; error = $_.Exception.ToString() }
                    Write-Host "$name $($_.Exception.Message)"
                }
                ConvertTo-Json -Depth 5 -InputObject $status | Set-Content -Encoding utf8 $statusPath
            }
        }
    }
} finally {
    $app.Quit()
    [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($app)
}
