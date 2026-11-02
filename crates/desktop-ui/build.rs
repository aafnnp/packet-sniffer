//! 把应用图标与版本信息嵌入 Windows 可执行文件。

fn main() {
    println!("cargo:rerun-if-changed=../../assets/icons/packet-sniffer.ico");

    #[cfg(windows)]
    {
        let mut resource = winresource::WindowsResource::new();
        resource.set_icon("../../assets/icons/packet-sniffer.ico");
        resource.set("ProductName", "Packet Sniffer");
        resource.set("FileDescription", "Packet Sniffer");
        resource.set("OriginalFilename", "packet-sniffer-ui.exe");
        resource.set("LegalCopyright", "MIT");
        resource.compile().expect("嵌入 Windows 图标与版本信息失败");
    }
}
