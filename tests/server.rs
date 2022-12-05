use super::*;

#[test]
fn run() {
  let rpc_server = test_bitcoincore_rpc::spawn();

  let port = TcpListener::bind("127.0.0.1:0")
    .unwrap()
    .local_addr()
    .unwrap()
    .port();

  let builder = CommandBuilder::new(format!("server --http-port {}", port)).rpc_server(&rpc_server);

  let mut command = builder.command();

  let mut child = command.spawn().unwrap();

  for attempt in 0.. {
    if let Ok(response) = reqwest::blocking::get(format!("http://localhost:{port}/status")) {
      if response.status() == 200 {
        assert_eq!(response.text().unwrap(), "OK");
        break;
      }
    }

    if attempt == 100 {
      panic!("Server did not respond to status check",);
    }

    thread::sleep(Duration::from_millis(50));
  }

  child.kill().unwrap();
}

#[test]
fn inscription_page() {
  let rpc_server = test_bitcoincore_rpc::spawn_with(Network::Regtest, "ord");
  let txid = rpc_server.mine_blocks(1)[0].txdata[0].txid();

  let stdout = CommandBuilder::new(format!(
    "--chain regtest wallet inscribe --satpoint {txid}:0:0 --file hello.txt"
  ))
  .write("hello.txt", "HELLOWORLD")
  .rpc_server(&rpc_server)
  .stdout_regex("commit\t[[:xdigit:]]{64}\nreveal\t[[:xdigit:]]{64}\n")
  .run();

  let reveal_tx = stdout.split("reveal\t").collect::<Vec<&str>>()[1].trim();

  rpc_server.mine_blocks(1);

  let ord_server = TestServer::spawn_with_args(&rpc_server, &[]);
  ord_server.assert_response_regex(
    &format!("/inscription/{}", reveal_tx),
    &format!(
      ".*<h1>Inscription</h1>
<dl>
  <dt>satpoint</dt>
  <dd>{reveal_tx}:0:0</dd>
</dl>
HELLOWORLD.*",
    ),
  );

  let ord_server = TestServer::spawn_with_args(&rpc_server, &[]);
  ord_server.assert_response_regex(
    &format!("/inscription/{}", reveal_tx),
    &format!(
      ".*<h1>Inscription</h1>
<dl>
  <dt>satpoint</dt>
  <dd>{reveal_tx}:0:0</dd>
</dl>
HELLOWORLD.*",
    ),
  )
}

#[test]
fn inscription_page_after_send() {
  let rpc_server = test_bitcoincore_rpc::spawn_with(Network::Regtest, "ord");
  let txid = rpc_server.mine_blocks(1)[0].txdata[0].txid();

  let stdout = CommandBuilder::new(format!(
    "--chain regtest wallet inscribe --satpoint {txid}:0:0 --file hello.txt"
  ))
  .write("hello.txt", "HELLOWORLD")
  .rpc_server(&rpc_server)
  .stdout_regex("commit\t[[:xdigit:]]{64}\nreveal\t[[:xdigit:]]{64}\n")
  .run();

  let reveal_txid = stdout.split("reveal\t").collect::<Vec<&str>>()[1].trim();

  rpc_server.mine_blocks(1);

  let ord_server = TestServer::spawn_with_args(&rpc_server, &[]);
  ord_server.assert_response_regex(
    &format!("/inscription/{}", reveal_txid),
    &format!(
      ".*<h1>Inscription</h1>
<dl>
  <dt>satpoint</dt>
  <dd>{reveal_txid}:0:0</dd>
</dl>
HELLOWORLD.*",
    ),
  );

  let txid = CommandBuilder::new(format!(
    "--chain regtest wallet send {reveal_txid}:0:0 bcrt1q6rhpng9evdsfnn833a4f4vej0asu6dk5srld6x"
  ))
  .write("hello.txt", "HELLOWORLD")
  .rpc_server(&rpc_server)
  .stdout_regex(".*")
  .run();

  rpc_server.mine_blocks(1);

  let ord_server = TestServer::spawn_with_args(&rpc_server, &[]);
  ord_server.assert_response_regex(
    &format!("/inscription/{}", reveal_txid),
    &format!(
      ".*<h1>Inscription</h1>
<dl>
  <dt>satpoint</dt>
  <dd>{}:0:0</dd>
</dl>
HELLOWORLD.*",
      txid.trim(),
    ),
  )
}