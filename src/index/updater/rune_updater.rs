use {
  super::*,
  crate::runes::{Edict, Runestone},
  serde_json::{Value, json},
};

struct Mint {
  id: RuneId,
  amount: u128,
}

struct Etched {
  divisibility: u8,
  id: RuneId,
  premine: u128,
  spaced_rune: SpacedRune,
  symbol: Option<char>,
  terms: Option<Terms>,
}

// pub(super) struct RuneTx {
//   pub block: u32,
//   pub txid: Txid,
//   pub tx_index: u32,
//   pub runestone: Runestone,
//   pub pointer: u32,
//   pub entries: HashMap<RuneId, RuneTxEntry>,
//   pub burned: HashMap<RuneId, u128>,
// }

// impl RuneTx {
//   pub fn new(
//       block: u32,
//       txid: Txid,
//       tx_index: u32,
//       runestone: Runestone,
//       pointer: u32,
//       entries: HashMap<RuneId, RuneTxEntry>,
//       burned: HashMap<RuneId, u128>,
//   ) -> Self {
//       Self {
//           block,
//           txid,
//           tx_index,
//           runestone,
//           pointer,
//           entries,
//           burned,
//       }
//   }
// }

#[derive(Debug, PartialEq, Copy, Clone, Serialize, Deserialize)]
pub(super) struct RuneTxEntry {
  pub burned: u128,
  pub divisibility: u8,
  pub etching: Txid,
  pub mints: u128,
  pub number: u64,
  pub premine: u128,
  pub spaced_rune: SpacedRune,
  pub symbol: Option<char>,
  pub terms: Option<Terms>,
  pub timestamp: u64,
  pub mintable: bool,
  pub supply: u128,
  pub start: Option<u64>,
  pub end: Option<u64>,
}

impl RuneTxEntry {

  fn load(rune_entry: &RuneEntry, current_block: u32) -> Self {
    Self {
      burned: rune_entry.burned,
      divisibility: rune_entry.divisibility,
      etching: rune_entry.etching,
      mints: rune_entry.mints,
      number: rune_entry.number,
      premine: rune_entry.premine,
      spaced_rune: rune_entry.spaced_rune,
      symbol: rune_entry.symbol,
      terms: rune_entry.terms,
      timestamp: rune_entry.timestamp,
      mintable: rune_entry.mintable(current_block as u64).is_ok(),
      supply: rune_entry.supply(),
      start: rune_entry.start(),
      end: rune_entry.end(),
    }
  }

}


pub(super) struct RuneUpdater<'a, 'tx, 'client> {
  pub(super) block_time: u32,
  pub(super) burned: HashMap<RuneId, u128>,
  pub(super) client: &'client Client,
  pub(super) height: u32,
  pub(super) id_to_entry: &'a mut Table<'tx, RuneIdValue, RuneEntryValue>,
  pub(super) inscription_id_to_sequence_number: &'a Table<'tx, InscriptionIdValue, u32>,
  pub(super) minimum: Rune,
  pub(super) outpoint_to_balances: &'a mut Table<'tx, &'static OutPointValue, &'static [u8]>,
  pub(super) rune_to_id: &'a mut Table<'tx, u128, RuneIdValue>,
  pub(super) runes: u64,
  pub(super) sequence_number_to_rune_id: &'a mut Table<'tx, u32, RuneIdValue>,
  pub(super) statistic_to_count: &'a mut Table<'tx, u64, u64>,
  pub(super) transaction_id_to_rune: &'a mut Table<'tx, &'static TxidValue, u128>,
}

impl<'a, 'tx, 'client> RuneUpdater<'a, 'tx, 'client> {
  pub(super) fn index_runes(&mut self, tx_index: u32, tx: &Transaction, txid: Txid, rune_txs: &mut Option<Vec<Value>>) -> Result {

    let runestone = Runestone::from_transaction(tx);
    if runestone.is_none() {
        return Ok(());
    }

    let mut unallocated = self.unallocated(tx)?;

    let cenotaph = runestone
      .as_ref()
      .map(|runestone| runestone.cenotaph)
      .unwrap_or_default();

    let pointer = runestone.as_ref().and_then(|runestone| runestone.pointer);

    let mut allocated: Vec<HashMap<RuneId, u128>> = vec![HashMap::new(); tx.output.len()];
    let mut entry_ids: HashSet<RuneId> = HashSet::new();

    if let Some(runestone) = runestone {

      if let Some(mint) = runestone
        .mint
        .and_then(|id| self.mint(id).transpose())
        .transpose()?
      {
        entry_ids.insert(mint.id);
        *unallocated.entry(mint.id).or_default() += mint.amount;
      }

      let etched = self.etched(tx_index, tx, &runestone)?;

      if let Some(Etched { id, premine, .. }) = etched {
        entry_ids.insert(id);
        *unallocated.entry(id).or_default() += premine;
      }

      if !cenotaph {
        for Edict { id, amount, output } in runestone.edicts {
          // edicts with output values greater than the number of outputs
          // should never be produced by the edict parser
          let output = usize::try_from(output).unwrap();
          assert!(output <= tx.output.len());

          let id = if id == RuneId::default() {
            let Some(Etched { id, .. }) = etched else {
              continue;
            };

            id
          } else {
            id
          };
          entry_ids.insert(id);

          let Some(balance) = unallocated.get_mut(&id) else {
            continue;
          };

          let mut allocate = |balance: &mut u128, amount: u128, output: usize| {
            if amount > 0 {
              *balance -= amount;
              *allocated[output].entry(id).or_default() += amount;
            }
          };

          if output == tx.output.len() {
            // find non-OP_RETURN outputs
            let destinations = tx
              .output
              .iter()
              .enumerate()
              .filter_map(|(output, tx_out)| {
                (!tx_out.script_pubkey.is_op_return()).then_some(output)
              })
              .collect::<Vec<usize>>();

            if amount == 0 {
              // if amount is zero, divide balance between eligible outputs
              let amount = *balance / destinations.len() as u128;
              let remainder = usize::try_from(*balance % destinations.len() as u128).unwrap();

              for (i, output) in destinations.iter().enumerate() {
                allocate(
                  balance,
                  if i < remainder { amount + 1 } else { amount },
                  *output,
                );
              }
            } else {
              // if amount is non-zero, distribute amount to eligible outputs
              for output in destinations {
                allocate(balance, amount.min(*balance), output);
              }
            }
          } else {
            // Get the allocatable amount
            let amount = if amount == 0 {
              *balance
            } else {
              amount.min(*balance)
            };

            allocate(balance, amount, output);
          }
        }
      }

      if let Some(etched) = etched {
        entry_ids.insert(etched.id);
        self.create_rune_entry(txid, cenotaph, etched)?;
      }
    }

    let mut burned: HashMap<RuneId, u128> = HashMap::new();
    let mut real_pointer: u32 = 0;

    if cenotaph {
      for (id, balance) in unallocated {
        entry_ids.insert(id);
        *burned.entry(id).or_default() += balance;
      }
    } else {
      // assign all un-allocated runes to the default output, or the first non
      // OP_RETURN output if there is no default, or if the default output is
      // too large
      if let Some(vout) = pointer
        .map(|pointer| pointer.into_usize())
        .inspect(|&pointer| assert!(pointer < allocated.len()))
        .or_else(|| {
          tx.output
            .iter()
            .enumerate()
            .find(|(_vout, tx_out)| !tx_out.script_pubkey.is_op_return())
            .map(|(vout, _tx_out)| vout)
        })
      {
        real_pointer = vout.try_into().unwrap();
        for (id, balance) in unallocated {
          entry_ids.insert(id);
          if balance > 0 {
            *allocated[vout].entry(id).or_default() += balance;
          }
        }
      } else {
        real_pointer = u32::MAX;
        for (id, balance) in unallocated {
          entry_ids.insert(id);
          if balance > 0 {
            *burned.entry(id).or_default() += balance;
          }
        }
      }
    }

    // update outpoint balances
    let mut buffer: Vec<u8> = Vec::new();
    for (vout, balances) in allocated.into_iter().enumerate() {
      if balances.is_empty() {
        continue;
      }

      // increment burned balances
      if tx.output[vout].script_pubkey.is_op_return() {
        for (id, balance) in &balances {
          *burned.entry(*id).or_default() += balance;
        }
        continue;
      }

      buffer.clear();

      let mut balances = balances.into_iter().collect::<Vec<(RuneId, u128)>>();

      // Sort balances by id so tests can assert balances in a fixed order
      balances.sort();

      for (id, balance) in balances {
        entry_ids.insert(id);
        id.encode_balance(balance, &mut buffer);
      }

      self.outpoint_to_balances.insert(
        &OutPoint {
          txid,
          vout: vout.try_into().unwrap(),
        }
        .store(),
        buffer.as_slice(),
      )?;
    }

    // increment entries with burned runes
    for (id, amount) in burned.clone() {
      entry_ids.insert(id);
      *self.burned.entry(id).or_default() += amount;
    }

    let outputs: Vec<Vec<(SpacedRune, Pile)>>  = tx.output.clone().into_iter()
    .enumerate()
    .map(|(vout, _)| {
      let x = self.get_rune_balances_for_outpoint(OutPoint {
        txid,
        vout: vout.try_into().unwrap(),
      }).unwrap();
      return x;
    }).collect();

    let rune_entries: HashMap<RuneId, RuneEntry> = entry_ids.into_iter().map(|id| {
      (id, RuneEntry::load(self.id_to_entry.get(&id.store()).unwrap().unwrap().value()))
    }).collect();

    let entries: HashMap<RuneId, RuneTxEntry> = rune_entries.iter().map(|(k, v)| (*k, RuneTxEntry::load(v, self.height))).collect();

    log::info!("Parsed rune tx: {:?}", json!({
      "block": self.height,
      "txid": txid,
      "tx_index": tx_index,
      "runestone": Runestone::from_transaction(tx),
      "pointer": real_pointer,
      "entries": entries,
      "burned": burned.clone(),
      "outputs": outputs
    }));

    if let Some(rune_txs) = rune_txs {
      rune_txs.push(json!({
        "block": self.height,
        "txid": txid,
        "tx_index": tx_index,
        "runestone": Runestone::from_transaction(tx),
        "pointer": real_pointer,
        "entries": entries,
        "burned": burned.clone(),
        "outputs": outputs
      }));
    }

    Ok(())
  }

  pub(super) fn get_rune_balances_for_outpoint(&mut self, outpoint: OutPoint,) -> Result<Vec<(SpacedRune, Pile)>>{
    let Some(balances) = self.outpoint_to_balances.get(&outpoint.store())? else {
      return Ok(Vec::new());
    };
    let balances_buffer = balances.value();

    let mut balances = Vec::new();
    let mut i = 0;
    while i < balances_buffer.len() {
      let ((id, amount), length) = RuneId::decode_balance(&balances_buffer[i..]).unwrap();
      i += length;

      let entry = RuneEntry::load(self.id_to_entry.get(id.store())?.unwrap().value());

      balances.push((
        entry.spaced_rune,
        Pile {
          amount,
          divisibility: entry.divisibility,
          symbol: entry.symbol,
        },
      ));
    }

    Ok(balances)
  }

  pub(super) fn update(self) -> Result {
    for (rune_id, burned) in self.burned {
      let mut entry = RuneEntry::load(self.id_to_entry.get(&rune_id.store())?.unwrap().value());
      entry.burned += burned;
      self.id_to_entry.insert(&rune_id.store(), entry.store())?;
    }

    Ok(())
  }

  fn create_rune_entry(&mut self, txid: Txid, burn: bool, etched: Etched) -> Result {
    let Etched {
      divisibility,
      id,
      premine,
      spaced_rune,
      symbol,
      terms,
    } = etched;

    self.rune_to_id.insert(spaced_rune.rune.0, id.store())?;
    self
      .transaction_id_to_rune
      .insert(&txid.store(), spaced_rune.rune.0)?;

    let number = self.runes;
    self.runes += 1;

    self
      .statistic_to_count
      .insert(&Statistic::Runes.into(), self.runes)?;

    self.id_to_entry.insert(
      id.store(),
      RuneEntry {
        block: id.block,
        burned: 0,
        divisibility,
        etching: txid,
        terms: terms.and_then(|terms| (!burn).then_some(terms)),
        mints: 0,
        number,
        premine,
        spaced_rune,
        symbol,
        timestamp: self.block_time.into(),
      }
      .store(),
    )?;

    let inscription_id = InscriptionId { txid, index: 0 };

    if let Some(sequence_number) = self
      .inscription_id_to_sequence_number
      .get(&inscription_id.store())?
    {
      self
        .sequence_number_to_rune_id
        .insert(sequence_number.value(), id.store())?;
    }

    Ok(())
  }

  fn etched(
    &mut self,
    tx_index: u32,
    tx: &Transaction,
    runestone: &Runestone,
  ) -> Result<Option<Etched>> {
    let Some(etching) = runestone.etching else {
      return Ok(None);
    };

    let rune = if let Some(rune) = etching.rune {
      if rune < self.minimum
        || rune.is_reserved()
        || self.rune_to_id.get(rune.0)?.is_some()
        || !self.tx_commits_to_rune(tx, rune)?
      {
        return Ok(None);
      }
      rune
    } else {
      let reserved_runes = self
        .statistic_to_count
        .get(&Statistic::ReservedRunes.into())?
        .map(|entry| entry.value())
        .unwrap_or_default();

      self
        .statistic_to_count
        .insert(&Statistic::ReservedRunes.into(), reserved_runes + 1)?;

      Rune::reserved(reserved_runes.into())
    };

    Ok(Some(Etched {
      divisibility: etching.divisibility.unwrap_or_default(),
      id: RuneId {
        block: self.height.into(),
        tx: tx_index,
      },
      premine: etching.premine.unwrap_or_default(),
      spaced_rune: SpacedRune {
        rune,
        spacers: etching.spacers.unwrap_or_default(),
      },
      symbol: etching.symbol,
      terms: etching.terms,
    }))
  }

  fn mint(&mut self, id: RuneId) -> Result<Option<Mint>> {
    let Some(entry) = self.id_to_entry.get(&id.store())? else {
      return Ok(None);
    };

    let mut rune_entry = RuneEntry::load(entry.value());

    let Ok(amount) = rune_entry.mintable(self.height.into()) else {
      return Ok(None);
    };

    drop(entry);

    rune_entry.mints += 1;

    self.id_to_entry.insert(&id.store(), rune_entry.store())?;

    Ok(Some(Mint { id, amount }))
  }

  fn tx_commits_to_rune(&self, tx: &Transaction, rune: Rune) -> Result<bool> {
    let commitment = rune.commitment();

    for input in &tx.input {
      // extracting a tapscript does not indicate that the input being spent
      // was actually a taproot output. this is checked below, when we load the
      // output's entry from the database
      let Some(tapscript) = input.witness.tapscript() else {
        continue;
      };

      for instruction in tapscript.instructions() {
        let instruction = instruction?;

        let Some(pushbytes) = instruction.push_bytes() else {
          continue;
        };

        if pushbytes.as_bytes() != commitment {
          continue;
        }

        let Some(tx_info) = self
          .client
          .get_raw_transaction_info(&input.previous_output.txid, None)
          .into_option()?
        else {
          panic!("input not in UTXO set: {}", input.previous_output);
        };

        let taproot = tx_info.vout[input.previous_output.vout.into_usize()]
          .script_pub_key
          .script()?
          .is_v1_p2tr();

        let mature = tx_info
          .confirmations
          .map(|confirmations| confirmations >= RUNE_COMMIT_INTERVAL)
          .unwrap_or_default();

        if taproot && mature {
          return Ok(true);
        }
      }
    }

    Ok(false)
  }

  fn unallocated(&mut self, tx: &Transaction) -> Result<HashMap<RuneId, u128>> {
    // map of rune ID to un-allocated balance of that rune
    let mut unallocated: HashMap<RuneId, u128> = HashMap::new();

    // increment unallocated runes with the runes in tx inputs
    for input in &tx.input {
      if let Some(guard) = self
        .outpoint_to_balances
        .remove(&input.previous_output.store())?
      {
        let buffer = guard.value();
        let mut i = 0;
        while i < buffer.len() {
          let ((id, balance), len) = RuneId::decode_balance(&buffer[i..]).unwrap();
          i += len;
          *unallocated.entry(id).or_default() += balance;
        }
      }
    }

    Ok(unallocated)
  }
}
