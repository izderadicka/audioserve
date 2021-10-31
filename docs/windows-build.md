### Windows build

**WARNING**: Windows are not officially supported - windows build instructions are contributed and resulting executable has some limitations (no features) and issues.

Clone the repository with:

    git clone https://github.com/izderadicka/audioserve

or download the ZIP archive.

Download the `x86_64-pc-windows-msvc` version of Rust compiler from [rust-lang.org](https://forge.rust-lang.org/infra/other-installation-methods.html#standalone). Also download and perform the default installation of the [Build tools for Visual Studio](https://visualstudio.microsoft.com/thank-you-downloading-visual-studio/?sku=BuildTools&rel=16). You may ignore the request to reboot the system after the installation.

Download the 4.1 "development" version of Windows 64-bit binaries of FFmpeg from [the official website](https://ffmpeg.zeranoe.com/builds/win64/dev/). Extract all `/lib/*.lib` files such as `avcodec.lib` from the archive to the `C:\Program Files\Rust stable MSVC 1.43\lib\rustlib\x86_64-pc-windows-msvc\lib` folder.

Open a Command prompt or a PowerShell window, change to the directory where you have previously extracted the contents of `audioserve-master` and run:

    cargo build --release --no-default-features

After compilation, you will find the compiled binary, `audioserve.exe`, in the `target\release` sub-folder.

Next, switch to the `client` folder under `\target\release`. Install the NPM software from https://nodejs.org/en/download/ and build the client using:

    npm install
    npm run build

Transfer the resulting `audioserve.exe` with the entire contents of the `client` folder to the preferred location.

#### Known issues:

- compilation with the `--features partially-static` option does not work (instead, use the shared FFmpeg libraries as described above).
- Audioserve doesn't recognize the paths that contain drive letters (i.e. `C:\`) and paths with symlinks or directory junctions. Put the `audioserve.exe` to the same disk drive as the folder with audio files and use it with paths _relative to the root_ of the drive. For example, if the path to the program is `d:\Audioserve\audioserve.exe`, its data folder is `D:\Audioserve\data` and your audio files are located in `C:\Audiobooks\`, launch the program as `D:\Audioserve\audioserve.exe --no-authentication --data-dir \Audioserve\data \Audiobooks`.
- As the result of the above, you can not use the multiple folders with audio files across the different disks with Audioserve.
- `Audioserve.exe` does not have an application icon.
- The program keeps the terminal window open while it is running. To hide it, use any Windows [utility](https://robotronic.de/runasserviceen.html) that allows launching terminal programs as "Windows services" in the background.
- Above instructions were only tested on a 64-bit Windows 10 platform.
