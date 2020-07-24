// Copyright 2020 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

mod account_storage;
mod blob_register;
mod elder_stores;
mod map_storage;
mod reading;
mod sequence_storage;
mod writing;

use crate::{
    node::keys::NodeKeys,
    node::msg_wrapping::ElderMsgWrapping,
    node::node_ops::{MessagingDuty, MetadataDuty, NodeDuty, NodeOperation},
    node::section_querying::SectionQuerying,
    node::state_db::NodeInfo,
    Result,
};
use account_storage::AccountStorage;
use blob_register::BlobRegister;
use elder_stores::ElderStores;
use map_storage::MapStorage;
use reading::Reading;
use safe_nd::{Cmd, ElderDuties, Message, MsgEnvelope, Query, XorName};
use sequence_storage::SequenceStorage;
use std::{
    cell::Cell,
    fmt::{self, Display, Formatter},
    rc::Rc,
};
use writing::Writing;

/// This module is called `Metadata`
/// as a preparation for the responsibilities
/// it will have eventually, after `Data Hierarchy Refinement`
/// has been implemented; where the data types are all simply
/// the structures + their metadata - handled at `Elders` - with
/// all underlying data being chunks stored at `Adults`.

pub struct Metadata {
    keys: NodeKeys,
    elder_stores: ElderStores,
    wrapping: ElderMsgWrapping,
}

impl Metadata {
    pub fn new(
        node_info: NodeInfo,
        total_used_space: &Rc<Cell<u64>>,
        section_querying: SectionQuerying,
    ) -> Result<Self> {
        let wrapping = ElderMsgWrapping::new(node_info.keys(), ElderDuties::Metadata);
        let account_storage =
            AccountStorage::new(node_info.clone(), total_used_space, wrapping.clone())?;
        let blob_register =
            BlobRegister::new(node_info.clone(), wrapping.clone(), section_querying)?;
        let map_storage = MapStorage::new(node_info.clone(), total_used_space, wrapping.clone())?;
        let sequence_storage =
            SequenceStorage::new(node_info.clone(), total_used_space, wrapping.clone())?;
        let elder_stores = ElderStores::new(
            account_storage,
            blob_register,
            map_storage,
            sequence_storage,
        );
        Ok(Self {
            keys: node_info.keys(),
            elder_stores,
            wrapping,
        })
    }

    pub fn process(&mut self, duty: &MetadataDuty) -> Option<NodeOperation> {
        use MetadataDuty::*;
        use NodeDuty::*;
        use NodeOperation::*;

        let result = match duty {
            ProcessRead(msg) | ProcessWrite(msg) => self.process_msg(msg),
        };

        result.map(|c| RunAsNode(ProcessMessaging(c)))
    }

    fn process_msg(&mut self, msg: &MsgEnvelope) -> Option<MessagingDuty> {
        match &msg.message {
            Message::Cmd {
                cmd: Cmd::Data { cmd, .. },
                ..
            } => {
                let mut writing = Writing::new(cmd.clone(), msg.clone());
                writing.get_result(&mut self.elder_stores)
            }
            Message::Query {
                query: Query::Data(query),
                ..
            } => {
                let reading = Reading::new(query.clone(), msg.clone());
                reading.get_result(&self.elder_stores)
            }
            _ => None, // only Queries and Cmds from client is handled at Metadata
        }
    }

    // This should be called whenever a node leaves the section. It fetches the list of data that was
    // previously held by the node and requests the other holders to store an additional copy.
    // The list of holders is also updated by removing the node that left.
    #[allow(unused)]
    pub fn trigger_chunk_duplication(&mut self, node: XorName) -> Option<Vec<MessagingDuty>> {
        self.elder_stores.blob_register_mut().duplicate_chunks(node)
    }

    // trace!(
    //     "{}: Received ({:?} {:?}) from src {:?} (client {:?})",
    //     self,
    //     request,
    //     message_id,
    //     src,
    //     requester
    // );

    // pub fn handle_response(
    //     &mut self,
    //     src: SrcLocation,
    //     response: Response,
    //     requester: PublicId,
    //     message_id: MessageId,
    //     proof: Option<(Request, Signature)>,
    // ) -> Option<MessagingDuty> {
    //     use Response::*;
    //     trace!(
    //         "{}: Received ({:?} {:?}) from {}",
    //         self,
    //         response,
    //         message_id,
    //         utils::get_source_name(src),
    //     );
    //     if let Some((request, signature)) = proof {
    //         if !matches!(requester, PublicId::Node(_))
    //             && self
    //                 .validate_section_signature(&request, &signature)
    //                 .is_none()
    //         {
    //             error!("Invalid section signature");
    //             return None;
    //         }
    //         match response {
    //             Write(result) => self.elder_stores.blob_register_mut().handle_write_result(
    //                 utils::get_source_name(src),
    //                 requester,
    //                 result,
    //                 message_id,
    //                 request,
    //             ),
    //             GetBlob(result) => self.elder_stores.blob_register().handle_get_result(
    //                 result,
    //                 message_id,
    //                 requester,
    //                 (request, signature),
    //             ),
    //             //
    //             // ===== Invalid =====
    //             //
    //             ref _other => {
    //                 error!(
    //                     "{}: Should not receive {:?} as a data handler.",
    //                     self, response
    //                 );
    //                 None
    //             }
    //         }
    //     } else {
    //         error!("Missing section signature");
    //         None
    //     }
    // }

    // fn initiate_duplication(
    //     &mut self,
    //     address: BlobAddress,
    //     holders: BTreeSet<XorName>,
    //     message_id: MessageId,
    //     accumulated_signature: Option<Signature>,
    // ) -> Option<MessagingDuty> {
    //     trace!(
    //         "Sending GetBlob request for address: ({:?}) to {:?}",
    //         address,
    //         holders,
    //     );
    //     let our_id = self.id.clone();
    //     wrap(MetadataCmd::SendToAdults {
    //         targets: holders,
    //         msg: Message::Request {
    //             request: Request::Node(NodeRequest::Read(Read::Blob(BlobRead::Get(address)))),
    //             requester: PublicId::Node(our_id),
    //             message_id,
    //             signature: Some((0, SignatureShare(accumulated_signature?))),
    //         },
    //     })
    // }

    // fn finalise_duplication(
    //     &mut self,
    //     sender: SrcLocation,
    //     response: Response,
    //     message_id: MessageId,
    //     Blob_address: BlobAddress,
    //     signature: Signature,
    // ) -> Option<MessagingDuty> {
    //     use Response::*;
    //     if self
    //         .routing
    //         .borrow()
    //         .public_key_set()
    //         .ok()?
    //         .public_key()
    //         .verify(&signature, &utils::serialise(&Blob_address))
    //     {
    //         match response {
    //             Write(result) => self.elder_stores.blob_register_mut().update_holders(
    //                 Blob_address,
    //                 utils::get_source_name(sender),
    //                 result,
    //                 message_id,
    //             ),
    //             // Duplication doesn't care about other type of responses
    //             ref _other => {
    //                 error!(
    //                     "{}: Should not receive {:?} as a data handler.",
    //                     self, response
    //                 );
    //                 None
    //             }
    //         }
    //     } else {
    //         error!("Ignoring duplication response. Invalid Signature.");
    //         None
    //     }
    // }

    // fn public_key(&self) -> Option<PublicKey> {
    //     Some(
    //         self.routing
    //             .borrow()
    //             .public_key_set()
    //             .ok()?
    //             .public_key(),
    //     )
    // }

    // fn validate_section_signature(&self, signature: &Signature) -> Option<()> {
    //     if self
    //         .public_key()?
    //         .verify(signature, &utils::serialise(request))
    //     {
    //         Some(())
    //     } else {
    //         None
    //     }
    // }
}

impl Display for Metadata {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        write!(formatter, "{}", self.keys.public_key())
    }
}