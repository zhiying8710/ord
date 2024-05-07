use {
  super::*, axum::{
    extract::{self, Extension, Path}, response::Json
  }, base64::{engine::general_purpose, Engine as _}, bitcoin::{consensus::ReadExt, hashes::hex::HexIterator, consensus::encode::Error as BtcError}, hex, serde_json::{json, to_string, Value}
};
use bitcoincore_rpc::Error as BtcRpcError;

fn deserialize_hex<T: Decodable>(hex: &str) -> Result<T> {
  let mut reader = HexIterator::new(&hex)?;
  let object = Decodable::consensus_decode(&mut reader)?;
  if reader.read_u8().is_ok() {
      Err(BtcRpcError::BitcoinSerialization(BtcError::ParseFailed(
          "data not consumed entirely when explicitly deserializing",
      )).into())
  } else {
      Ok(object)
  }
}


pub(super) struct Rest {

}


impl Rest {

  pub async fn inscription(
    Extension(server_config): Extension<Arc<ServerConfig>>,
    Extension(index): Extension<Arc<Index>>,
    Path(DeserializeFromStr(query)): Path<DeserializeFromStr<query::Inscription>>,
  ) -> ServerResult<Json<Value>>{

    Ok(
      Json(_get_inscription(&server_config, &index, query)?)
    )
  }

  pub async fn inscriptions(
    Extension(server_config): Extension<Arc<ServerConfig>>,
    Extension(index): Extension<Arc<Index>>,
    extract::Json(inscription_ids): extract::Json<Vec<DeserializeFromStr<query::Inscription>>>,
  ) -> ServerResult<Json<Vec<Value>>>{
    let mut _inscriptions: Vec<Value> = vec![];

    for inscription_id in inscription_ids {
      let inscription = _get_inscription(&server_config, &index, inscription_id.0)?;
      _inscriptions.push(inscription);
    }

    Ok(
      Json(_inscriptions)
    )
  }

  pub async fn sat(
    Extension(server_config): Extension<Arc<ServerConfig>>,
    Extension(index): Extension<Arc<Index>>,
    Path(DeserializeFromStr(sat)): Path<DeserializeFromStr<Sat>>,
  ) -> ServerResult<Json<Value>> {
    let satpoint = index.rare_sat_satpoint(sat)?;
    Ok(
      Json(json!({
        "sat": sat.n().to_string(),
        "decimal": sat.decimal().to_string(),
        "degree": sat.degree().to_string(),
        "percentile": sat.percentile(),
        "name": sat.name(),
        "cycle": sat.cycle(),
        "epoch": sat.epoch().to_string(),
        "period": sat.period(),
        "block": sat.height().to_string(),
        "offset": sat.third(),
        "rarity": sat.rarity(),
        "satpoint": satpoint.unwrap_or(SatPoint {
          outpoint: OutPoint::null(),
            offset: 0,
          }),
        "blocktime": index.block_time(sat.height())?.timestamp().timestamp(),
        // "inscription": index.get_inscription_id_by_sat(sat)?
      }))
    )
  }

  pub fn _parse_inscriptions_from_witness(witnesses: Vec<Witness>) -> Vec<Value> {
    return ParsedEnvelope::from_witness(witnesses).into_iter().map(|envelope| {
      let inscription = envelope.payload;
      let mut content: String = "".to_owned();
      if let Some(_body) = inscription.clone().into_body() {
        content = general_purpose::STANDARD.encode(&_body);
      }
      json!({
        "content_length": inscription.content_length().unwrap_or(0),
        "content_type": inscription.content_type().unwrap_or(""),
        "content": content,
        "content_encoding": inscription.content_encoding().map_or(String::new(), |header_value| header_value.to_str().unwrap_or("").to_string()),
        "metaprotocol": inscription.metaprotocol(),
        "metadata":  match inscription.metadata() {
          Some(meta) => to_string(&meta).unwrap(),
          _ => "{}".to_owned(),
        },
        "parents": inscription.parents(),
        "delegate": inscription.delegate(),
        "unrecognized_even_field": inscription.unrecognized_even_field,
        "duplicate_field": inscription.duplicate_field,
        "incomplete_field": inscription.incomplete_field,
        "first_input": envelope.input == 0,
        "first_sat": envelope.offset == 0,
        "pointer": inscription.pointer.is_some(),
        "pushnum": envelope.pushnum,
        "stutter": envelope.stutter,
        "input": envelope.input,
        "offset": envelope.offset,
      })
    }).collect();
  }

  pub async fn parse_inscriptions_from_witness(
    Extension(server_config): Extension<Arc<ServerConfig>>,
    Extension(index): Extension<Arc<Index>>,
    extract::Json(witnesses): extract::Json<Vec<Vec<String>>>
  ) -> ServerResult<Json<Vec<Value>>> {
    let witnesses: Vec<Witness> = witnesses.iter().map(|witness| {
      let x: Vec<Vec<u8>> = witness.into_iter().map(|s|hex::decode(s).unwrap()).collect();
      return Witness::from(x);
    }).collect();
    Ok(
      Json(Self::_parse_inscriptions_from_witness(witnesses))
    )
  }

  pub async fn parse_inscriptions(
    Extension(server_config): Extension<Arc<ServerConfig>>,
    Extension(index): Extension<Arc<Index>>,
    Path(txid): Path<Txid>,
  ) -> ServerResult<Json<Vec<Value>>> {
    let tx = index.get_raw_transaction(txid)?.unwrap();
    let witnesses = tx.input.iter().map(|input| input.clone().witness).collect();
    Ok(
      Json(Self::_parse_inscriptions_from_witness(witnesses))
    )
  }

  pub async fn parse_rune(
    Extension(server_config): Extension<Arc<ServerConfig>>,
    Extension(index): Extension<Arc<Index>>,
    Path(txid): Path<Txid>,
  ) -> ServerResult<Json<Option<Artifact>>> {
    let tx = index.get_raw_transaction(txid)?.unwrap();
    Ok(
      Json(Runestone::decipher(&tx))
    )
  }

  fn _output(
    outpoint: OutPoint,
    index: &Arc<Index>,
    chain: Chain
   ) -> Result<Value> {
    let sat_ranges = index.list(outpoint)?;

      let indexed;

      let output = if outpoint == OutPoint::null() || outpoint == unbound_outpoint() {
        let mut value = 0;

        if let Some(ranges) = &sat_ranges {
          for (start, end) in ranges {
            value += end - start;
          }
        }

        indexed = true;

        TxOut {
          value,
          script_pubkey: ScriptBuf::new(),
        }
      } else {
        indexed = index.contains_output(&outpoint)?;

        index
          .get_transaction(outpoint.txid).unwrap()
          .ok_or_not_found(|| format!("output {outpoint}")).unwrap()
          .output
          .into_iter()
          .nth(outpoint.vout as usize)
          .ok_or_not_found(|| format!("output {outpoint}")).unwrap()
      };

      let inscriptions = index.get_inscriptions_on_output(outpoint)?;

      let runes = index.get_rune_balances_for_outpoint(outpoint)?;

      let spent = index.is_output_spent(outpoint)?;
      let output = api::Output::new(
        chain,
        inscriptions,
        outpoint,
        output,
        indexed,
        runes,
        sat_ranges,
        spent,
      );
      Ok(json!({
        "address": output.address,
        "inscriptions": output.inscriptions,
        "outpoint": outpoint,
        "ranges": output.sat_ranges,
        "runes": output.runes
      }))
  }


  pub async fn outputs(
    Extension(server_config): Extension<Arc<ServerConfig>>,
    Extension(index): Extension<Arc<Index>>,
    extract::Json(outpoints): extract::Json<Vec<OutPoint>>,
  ) -> ServerResult<Json<Vec<Value>>> {
    let mut _outputs = vec![];

    for outpoint in outpoints {
      _outputs.push(Self::_output(outpoint, &index, server_config.chain)?);
    }
    Ok(Json(_outputs))
  }

  pub async fn parse_rune_from_hex(
    Extension(server_config): Extension<Arc<ServerConfig>>,
    Extension(index): Extension<Arc<Index>>,
    tx_hex: String,
  ) -> ServerResult<Json<Option<Artifact>>> {
    let tx: Transaction = deserialize_hex(&tx_hex).unwrap();
    Ok(Json(Runestone::decipher(&tx)))
  }

}

fn _get_inscription(
  server_config: &Arc<ServerConfig>,
  index: &Arc<Index>,
  query: query::Inscription,
) -> Result<Value, ServerError>{
  let (api_inscription, tx_out, _) = Index::inscription_info(&index, query)?.ok_or_not_found(|| format!("inscription {query}"))?;

  let raw_tx = index.get_raw_transaction(api_inscription.id.txid)?;

  let current_inscription : Inscription = raw_tx.and_then(|tx| {
    ParsedEnvelope::from_transaction(&tx, &Some("".to_string()))
    .into_iter()
    .nth(api_inscription.id.index as usize)
    .map(|envelope| envelope.payload)
  }).unwrap();

  let mut content: String = "".to_owned();
  if let Some(_body) = current_inscription.clone().into_body() {
    content = general_purpose::STANDARD.encode(&_body);
  }

  Ok(
    json!({
      "genesis_fee": api_inscription.fee,
      "genesis_height": api_inscription.height,
      "content_length": api_inscription.content_length,
      "content_type": api_inscription.content_type,
      "content": content,
      "inscription_id": api_inscription.id,
      "charms": api_inscription.charms,
      "address": tx_out.as_ref()
      .and_then(|o| {
        server_config
          .chain
          .address_from_script(&o.script_pubkey)
          .ok()
      })
      .map(|address| address.to_string()),
      "number": api_inscription.number,
      "output": json!({
        "value": tx_out.as_ref().map(|o| o.value)
      }),
      "sat": api_inscription.sat,
      "satpoint": api_inscription.satpoint,
      "metadata": match current_inscription.clone().metadata() {
        Some(meta) => match to_string(&meta) {
          Ok(v) => v,
          Err(_) => "{}".to_owned(),
        },
        _ => "{}".to_owned(),
      },
      "metaprotocol": current_inscription.metaprotocol(),
      "timestamp": api_inscription.timestamp,
     })
  )
}

#[test]
fn test_rune_op_return() {
  let tx_hex = "02000000000101fa6b70a1340130c92dbf5fc63309d4681c616838a908d0e7d9c07b49be6582460000000000fdffffff0310270000000000002251205181d6944712849ae0bb4bf560663d6f924eb166e70fed78a95fc8c9c9f83bbf102700000000000022512058903486c396b28d0ace8d43f30d37f1c810b231ad3079ae1a1e13647d2c66080000000000000000306a5d2d02030484ded3a5b58a8285fac8a0fc8e080388800205b12d06e8070ae80708808088fccdbcc323000000e807010340453c8cb474abfdd01b3fb3f7966e5cde7dbbb54d0a531d2a6f7796e511c241b024f99c7ace6822a66d8beb12a7ef9bcc067d1c9ec5d13060c4a8b7e1269df3a15c208d6d0667c2ecfd0cf60e80e3fa848d22bde4e5a63ff2cd879d0456368e9c0d40ac0063036f7264010118746578742f706c61696e3b636861727365743d7574662d38010200010d0c04efb45453080a7a2488ef400004e19ab10a6821c18d6d0667c2ecfd0cf60e80e3fa848d22bde4e5a63ff2cd879d0456368e9c0d4000000000";
  let tx: Transaction = deserialize_hex(tx_hex).unwrap();
  println!("{:?}", Runestone::decipher(&tx))
}
