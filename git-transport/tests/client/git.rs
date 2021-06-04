#[cfg(all(not(feature = "blocking-client"), feature = "async-client"))]
use futures_lite::{AsyncBufReadExt, AsyncWriteExt, StreamExt};
#[cfg(feature = "blocking-client")]
use std::io::{BufRead, Write};

use std::ops::Deref;

use bstr::ByteSlice;

use git_transport::{
    client,
    client::{git, Transport},
    Protocol, Service,
};

use crate::fixture_bytes;
use std::sync::{Arc, Mutex};

#[maybe_async::test(feature = "blocking-client", async(feature = "async-client", async_std::test))]
async fn handshake_v1_and_request() -> crate::Result {
    let mut out = Vec::new();
    let server_response = fixture_bytes("v1/clone.response");
    let mut c = git::Connection::new(
        server_response.as_slice(),
        &mut out,
        Protocol::V1,
        "/foo.git",
        Some(("example.org", None)),
        git::ConnectMode::Daemon,
    );
    assert!(c.is_stateful(), "tcp connections are stateful");
    assert_eq!(c.to_url(), "file:///foo.git");
    let mut res = c.handshake(Service::UploadPack).await?;
    assert_eq!(res.actual_protocol, Protocol::V1);
    assert_eq!(
        res.capabilities
            .iter()
            .map(|c| (c.name().to_owned(), c.value().map(ToOwned::to_owned)))
            .collect::<Vec<_>>(),
        [
            ("multi_ack", None),
            ("thin-pack", None),
            ("side-band", None),
            ("side-band-64k", None),
            ("ofs-delta", None),
            ("shallow", None),
            ("deepen-since", None),
            ("deepen-not", None),
            ("deepen-relative", None),
            ("no-progress", None),
            ("include-tag", None),
            ("multi_ack_detailed", None),
            ("symref", Some("HEAD:refs/heads/master")),
            ("object-format", Some("sha1")),
            ("agent", Some("git/2.28.0"))
        ]
        .iter()
        .map(|(n, v)| (
            n.as_bytes().as_bstr().to_owned(),
            v.map(|v| v.as_bytes().as_bstr().to_owned())
        ))
        .collect::<Vec<_>>()
    );
    let mut lines = res.refs.as_mut().expect("v1 protocol provides refs").lines();
    let mut refs = Vec::new();
    while let Some(line) = lines.next().await {
        refs.push(line?)
    }

    assert_eq!(
        refs,
        vec![
            "808e50d724f604f69ab93c6da2919c014667bedb HEAD",
            "808e50d724f604f69ab93c6da2919c014667bedb refs/heads/master"
        ]
    );
    drop(res);

    let writer = c.request(client::WriteMode::Binary, client::MessageKind::Flush)?;
    let nak_line = writer
        .into_read()
        .await?
        .lines()
        .next()
        .await
        .expect("exactly one line")?;
    assert_eq!(nak_line, "NAK");

    let mut writer = c.request(
        client::WriteMode::OneLfTerminatedLinePerWriteCall,
        client::MessageKind::Text(b"done"),
    )?;

    writer.write_all(b"hello").await?;
    writer.write_all(b"world").await?;

    let mut reader = writer.into_read().await?;
    let messages = Arc::new(Mutex::new(Vec::<String>::new()));
    reader.set_progress_handler(Some(Box::new({
        let sb = messages.clone();
        move |is_err, data| {
            assert!(!is_err);
            sb.deref()
                .lock()
                .expect("no poison")
                .push(std::str::from_utf8(data).expect("valid utf8").to_owned())
        }
    })));

    let expected_entries = 3;
    #[cfg(feature = "blocking-client")]
    {
        use git_pack::data::input;
        let entries = git_pack::data::input::BytesToEntriesIter::new_from_header(
            reader,
            input::Mode::Verify,
            input::EntryDataMode::Crc32,
        )?;
        assert_eq!(entries.count(), expected_entries);
    }
    // In async mode, show that we can indeed
    #[cfg(all(not(feature = "blocking-client"), feature = "async-client"))]
    {
        use git_pack::data::input;
        let entries = git_pack::data::input::BytesToEntriesIter::new_from_header(
            futures_lite::io::BlockOn::new(reader),
            input::Mode::Verify,
            input::EntryDataMode::Crc32,
        )?;
        assert_eq!(entries.count(), expected_entries);
    }

    let sidebands = Arc::try_unwrap(messages)
        .expect("no other handle")
        .into_inner()
        .expect("no poison");
    assert_eq!(sidebands.len(), 6, "…along with some status messages");
    c.close().await?;

    assert_eq!(
        out.as_slice().as_bstr(),
        b"002egit-upload-pack /foo.git\0host=example.org\00000000ahello\n\
    000aworld\n\
    0009done\n0000"
            .as_bstr(),
        "it sends the correct request"
    );
    Ok(())
}

#[maybe_async::test(feature = "blocking-client", async(feature = "async-client", async_std::test))]
async fn handshake_v1_process_mode() -> crate::Result {
    let mut out = Vec::new();
    let server_response = fixture_bytes("v1/clone.response");
    let mut c = git::Connection::new(
        server_response.as_slice(),
        &mut out,
        Protocol::V1,
        "/foo.git",
        Some(("example.org", None)),
        git::ConnectMode::Process,
    );
    c.handshake(Service::UploadPack).await?;

    assert_eq!(
        out.as_slice().as_bstr(),
        b"".as_bstr(),
        "it sends no introductory line to help the daemon start the right thing"
    );
    Ok(())
}