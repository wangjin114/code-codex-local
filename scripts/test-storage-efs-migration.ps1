[CmdletBinding()]
param([Parameter(Mandatory=$true)][string]$EncryptedSource)
$ErrorActionPreference='Stop'
$repo=Split-Path -Parent $PSScriptRoot
. (Join-Path $repo 'installer\Storage-Layout.ps1')
$source=Get-Item -LiteralPath $EncryptedSource
if($source.PSIsContainer-or ($source.Attributes-band [IO.FileAttributes]::ReparsePoint)-or -not ($source.Attributes-band [IO.FileAttributes]::Encrypted)){throw 'A real regular encrypted source is required'}
$before=Get-CodeCodexDataHash $source.FullName
$fixture=Join-Path $repo ('artifacts\efs-migration-'+[Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $fixture | Out-Null
$acl=Get-Acl -LiteralPath $fixture
$acl.SetAccessRuleProtection($true,$false)
foreach($rule in @($acl.Access)){$null=$acl.RemoveAccessRuleSpecific($rule)}
foreach($sid in @([Security.Principal.WindowsIdentity]::GetCurrent().User,[Security.Principal.SecurityIdentifier]::new('S-1-5-18'),[Security.Principal.SecurityIdentifier]::new('S-1-5-32-544'))){$acl.AddAccessRule([Security.AccessControl.FileSystemAccessRule]::new($sid,'FullControl','ContainerInherit,ObjectInherit','None','Allow'))}
Set-Acl -LiteralPath $fixture -AclObject $acl
$destination=Join-Path $fixture 'migrated.dat'
$script:MigrationCopied=0;$script:MigrationSkipped=0
try {
Copy-CodeCodexDataTree $source.FullName $destination
if((Get-CodeCodexDataHash $destination)-ne $before){throw 'Encrypted-source migration changed bytes'}
if((Get-Item -LiteralPath $destination).Attributes-band [IO.FileAttributes]::Encrypted){throw 'Migration propagated source encryption into the regular destination'}
if((Get-CodeCodexDataHash $source.FullName)-ne $before-or -not ((Get-Item -LiteralPath $source.FullName).Attributes-band [IO.FileAttributes]::Encrypted)){throw 'Original encrypted source changed'}
[IO.File]::WriteAllText($destination,'newer destination wins')
Copy-CodeCodexDataTree $source.FullName $destination
if([IO.File]::ReadAllText($destination)-ne 'newer destination wins'-or $script:MigrationSkipped-ne 1-or $script:MigrationCopied-ne 1){throw 'Repeated migration did not preserve the newer destination'}
if(@(Get-ChildItem -LiteralPath $fixture -Filter '*.tmp').Count){throw 'Migration left temporary files'}
$failureSource=Join-Path $fixture 'failure-source.dat'
$failureDestination=Join-Path $fixture 'failure-destination.dat'
[IO.File]::WriteAllText($failureSource,'copy failure must not publish or lock files')
$originalHashFunction=(Get-Command Get-CodeCodexDataHash).ScriptBlock
$script:realMigrationHash=$originalHashFunction
try {
 function Get-CodeCodexDataHash([string]$Path){if($Path.EndsWith('.tmp')){throw [IO.IOException]::new('Injected verification read failure')}; & $script:realMigrationHash $Path}
 $failed=$false
 try{Copy-CodeCodexDataTree $failureSource $failureDestination}catch{$failed=$_.Exception.Message-eq 'Injected verification read failure'}
 if(!$failed-or (Test-Path -LiteralPath $failureDestination)-or @(Get-ChildItem -LiteralPath $fixture -Filter '*.tmp').Count){throw 'Failed verification published data or left a stage'}
 $exclusive=[IO.File]::Open($failureSource,[IO.FileMode]::Open,[IO.FileAccess]::ReadWrite,[IO.FileShare]::None);$exclusive.Dispose()
 if([IO.File]::ReadAllText($failureSource)-ne 'copy failure must not publish or lock files'){throw 'Failed verification changed source bytes'}
}finally{Set-Item -Path function:Get-CodeCodexDataHash -Value $originalHashFunction}
[pscustomobject]@{Passed=7;EncryptedSourceRetained=$true;BytesVerified=$true;DestinationPreserved=$true;FailureUnpublished=$true;HandlesReleased=$true;NoTemporaryFiles=$true;Fixture=$fixture} | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $fixture 'result.json') -Encoding utf8
Write-Output 'EFS migration passed: 7 checks including verification failure cleanup and handle release.'
}finally{
 foreach($file in Get-ChildItem -LiteralPath $fixture -File -Force){
  if($file.Name-in @('migrated.dat','failure-source.dat','failure-destination.dat')-or $file.Name-match '^(migrated|failure-destination)\.dat\.[a-f0-9]{32}\.tmp$'){
   if($file.Attributes-band [IO.FileAttributes]::ReparsePoint){throw 'Redirected test cleanup refused'}
   Remove-Item -LiteralPath $file.FullName -Force
  }
 }
}
