# Read only: never resolve, save, or repair the shortcut.
function Get-ShortcutMetadata([string]$Path) {
    $bytes = [IO.File]::ReadAllBytes($Path)
    if ($bytes.Length -lt 80 -or [BitConverter]::ToUInt32($bytes, 0) -ne 76 -or
        [guid]::new([byte[]]$bytes[4..19]) -ne [guid]'00021401-0000-0000-C000-000000000046') {
        throw "Invalid Shell Link header: $Path"
    }
    $flags = [BitConverter]::ToUInt32($bytes, 20)
    $offset = 76
    $idListSize = 0
    $linkInfoSize = 0
    if ($flags -band 1) {
        $idListSize = [BitConverter]::ToUInt16($bytes, $offset)
        $offset += 2 + $idListSize
    }
    if ($flags -band 2) {
        $linkInfoSize = [BitConverter]::ToUInt32($bytes, $offset)
        if ($linkInfoSize -lt 28) { throw 'Invalid LinkInfo size.' }
        $offset += $linkInfoSize
    }
    $strings = [ordered]@{}
    foreach ($entry in @(@(4, 'Name'), @(8, 'RelativePath'), @(16, 'WorkingDirectory'), @(32, 'Arguments'), @(64, 'IconLocation'))) {
        if ($flags -band $entry[0]) {
            $length = [BitConverter]::ToUInt16($bytes, $offset)
            $offset += 2
            $size = $length
            $encoding = [Text.Encoding]::Default
            if ($flags -band 128) { $size *= 2; $encoding = [Text.Encoding]::Unicode }
            if ($offset + $size -gt $bytes.Length) { throw 'Truncated StringData.' }
            $strings[$entry[1]] = $encoding.GetString($bytes, $offset, $size)
            $offset += $size
        }
    }
    $blocks = @()
    while ($true) {
        if ($offset + 4 -gt $bytes.Length) { throw 'Missing ExtraData terminator.' }
        $size = [BitConverter]::ToUInt32($bytes, $offset)
        if ($size -eq 0) { break }
        if ($size -lt 8 -or $offset + $size -gt $bytes.Length) { throw 'Invalid ExtraData block.' }
        $blocks += [ordered]@{ Signature = '0x{0:X8}' -f [BitConverter]::ToUInt32($bytes, $offset + 4); Size = $size }
        $offset += $size
    }
    $link = (New-Object -ComObject WScript.Shell).CreateShortcut($Path)
    $item = (New-Object -ComObject Shell.Application).Namespace((Split-Path $Path)).ParseName((Split-Path $Path -Leaf))
    [pscustomobject][ordered]@{
        Path = $Path
        Hash = (Get-FileHash -LiteralPath $Path).Hash
        TargetPath = $link.TargetPath
        WorkingDirectory = $link.WorkingDirectory
        IconLocation = $link.IconLocation
        Arguments = $link.Arguments
        Description = $link.Description
        AppUserModelID = $item.ExtendedProperty('System.AppUserModel.ID')
        LinkFlags = '0x{0:X8}' -f $flags
        FileAttributes = '0x{0:X8}' -f [BitConverter]::ToUInt32($bytes, 24)
        IconIndex = [BitConverter]::ToInt32($bytes, 56)
        ShowCommand = [BitConverter]::ToUInt32($bytes, 60)
        HotKey = [BitConverter]::ToUInt16($bytes, 64)
        IDListSize = $idListSize
        LinkInfoSize = $linkInfoSize
        StringData = $strings
        ExtraData = $blocks
        TrailingBytes = $bytes.Length - $offset - 4
    }
}
