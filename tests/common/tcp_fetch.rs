use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};

// Fetch using a raw `TCP` connection.
pub async fn tcp_fetch(url: &str) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let (authority, path) = split_url(url);
    let mut parts = authority.split(':');
    let host = parts.next().unwrap_or(authority);
    let port = parts
        .next()
        .and_then(|port| port.parse::<u16>().ok())
        .unwrap_or(80);

    let mut stream = TcpStream::connect(format!("{}:{}", host, port)).await?;

    // Fetch the url.
    let request = format!(
        "GET {} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\r\n",
        path, authority
    );
    stream.write_all(request.as_bytes()).await?;

    // Asynchronously read the response until the connection is closed.
    let mut response = Vec::new();
    stream.read_to_end(&mut response).await?;
    let response = String::from_utf8(response)?;

    // Take the body of the response.
    let body = response
        .split_once("\r\n\r\n")
        .map(|(_, body)| body)
        .ok_or("could not find the body of the response")?;

    Ok(body.to_owned())
}

fn split_url(url: &str) -> (&str, &str) {
    let url = url
        .strip_prefix("http://")
        .or_else(|| url.strip_prefix("https://"))
        .unwrap_or(url);

    url.find('/')
        .map_or((url, "/"), |index| (&url[..index], &url[index..]))
}
