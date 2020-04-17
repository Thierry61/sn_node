// Copyright 2020 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use super::Responder;
use crate::{
    action::{Action, ConsensusAction},
    client_handler::auth_keys::AuthKeysDb,
    rpc::Rpc,
    utils::{self, AuthorisationKind},
};
use log::{error, warn};
use safe_nd::{
    AppPermissions, AppPublicId, Coins, Error as NdError, MessageId, NodePublicId, PublicId,
    PublicKey, Request, Response, Signature,
};
use std::{
    cell::RefCell,
    fmt::{self, Display, Formatter},
    rc::Rc,
};

#[derive(Clone, Debug)]
pub struct ClientInfo {
    pub public_id: PublicId,
}

pub(super) struct Auth {
    id: NodePublicId,
    auth_keys: AuthKeysDb,
    responder: Rc<RefCell<Responder>>,
}

impl Auth {
    pub fn new(id: NodePublicId, auth_keys: AuthKeysDb, responder: Rc<RefCell<Responder>>) -> Self {
        Self {
            id,
            auth_keys,
            responder,
        }
    }

    // If the client is app, check if it is authorised to perform the given request.
    pub fn authorise_app(
        &mut self,
        public_id: &PublicId,
        request: &Request,
        message_id: MessageId,
    ) -> Option<()> {
        let app_id = match public_id {
            PublicId::App(app_id) => app_id,
            _ => return Some(()),
        };

        let result = match utils::authorisation_kind(request) {
            AuthorisationKind::GetPub => Ok(()),
            AuthorisationKind::GetUnpub => self.check_app_permissions(app_id, |_| true),
            AuthorisationKind::GetBalance => {
                self.check_app_permissions(app_id, |perms| perms.get_balance)
            }
            AuthorisationKind::Mut => {
                self.check_app_permissions(app_id, |perms| perms.perform_mutations)
            }
            AuthorisationKind::TransferCoins => {
                self.check_app_permissions(app_id, |perms| perms.transfer_coins)
            }
            AuthorisationKind::MutAndTransferCoins => self.check_app_permissions(app_id, |perms| {
                perms.transfer_coins && perms.perform_mutations
            }),
            AuthorisationKind::ManageAppKeys => Err(NdError::AccessDenied),
        };

        if let Err(error) = result {
            self.responder
                .borrow_mut()
                .respond_to_client(message_id, request.error_response(error));
            None
        } else {
            Some(())
        }
    }

    // client query
    pub fn list_auth_keys_and_version(
        &mut self,
        client: &ClientInfo,
        message_id: MessageId,
    ) -> Option<Action> {
        let result = Ok(self
            .auth_keys
            .list_auth_keys_and_version(utils::client(&client.public_id)?));

        self.responder
            .borrow_mut()
            .respond_to_client(message_id, Response::ListAuthKeysAndVersion(result));
        None
    }

    // on client request
    pub fn initiate_auth_key_insertion(
        &self,
        client: &ClientInfo,
        key: PublicKey,
        new_version: u64,
        permissions: AppPermissions,
        message_id: MessageId,
    ) -> Option<Action> {
        Some(Action::VoteFor(ConsensusAction::Forward {
            request: Request::InsAuthKey {
                key,
                version: new_version,
                permissions,
            },
            client_public_id: client.public_id.clone(),
            message_id,
            cost: Coins::from_nano(0),
        }))
    }

    // on consensus
    pub fn finalize_auth_key_insertion(
        &mut self,
        client: PublicId,
        key: PublicKey,
        new_version: u64,
        permissions: AppPermissions,
        message_id: MessageId,
    ) -> Option<Action> {
        let result =
            self.auth_keys
                .ins_auth_key(utils::client(&client)?, key, new_version, permissions);
        Some(Action::RespondToClientHandlers {
            sender: *self.id.name(),
            rpc: Rpc::Response {
                response: Response::Mutation(result),
                requester: client,
                message_id,
            },
        })
    }

    // on client request
    pub fn initiate_auth_key_deletion(
        &mut self,
        client: &ClientInfo,
        key: PublicKey,
        new_version: u64,
        message_id: MessageId,
    ) -> Option<Action> {
        Some(Action::VoteFor(ConsensusAction::Forward {
            request: Request::DelAuthKey {
                key,
                version: new_version,
            },
            client_public_id: client.public_id.clone(),
            message_id,
            cost: Coins::from_nano(0),
        }))
    }

    // on consensus
    pub fn finalize_auth_key_deletion(
        &mut self,
        client: PublicId,
        key: PublicKey,
        new_version: u64,
        message_id: MessageId,
    ) -> Option<Action> {
        let result = self
            .auth_keys
            .del_auth_key(utils::client(&client)?, key, new_version);
        Some(Action::RespondToClientHandlers {
            sender: *self.id.name(),
            rpc: Rpc::Response {
                response: Response::Mutation(result),
                requester: client,
                message_id,
            },
        })
    }

    // Verify that valid signature is provided if the request requires it.
    pub fn verify_signature(
        &mut self,
        public_id: &PublicId,
        request: &Request,
        message_id: MessageId,
        signature: Option<Signature>,
    ) -> Option<()> {
        let signature_required = match utils::authorisation_kind(request) {
            AuthorisationKind::GetUnpub
            | AuthorisationKind::GetBalance
            | AuthorisationKind::TransferCoins
            | AuthorisationKind::Mut
            | AuthorisationKind::MutAndTransferCoins
            | AuthorisationKind::ManageAppKeys => true,
            AuthorisationKind::GetPub => false,
        };

        if !signature_required {
            return Some(());
        }

        let valid = if let Some(signature) = signature {
            self.is_valid_client_signature(public_id, request, &message_id, &signature)
        } else {
            warn!(
                "{}: ({:?}/{:?}) from {} is unsigned",
                self, request, message_id, public_id
            );
            false
        };

        if valid {
            Some(())
        } else {
            self.responder.borrow_mut().respond_to_client(
                message_id,
                request.error_response(NdError::InvalidSignature),
            );
            None
        }
    }

    pub fn verify_consistent_address(
        &mut self,
        request: &Request,
        message_id: MessageId,
    ) -> Option<()> {
        use Request::*;
        let consistent = match request {
            AppendSeq { ref append, .. } => append.address.is_seq(),
            AppendUnseq(ref append) => !&append.address.is_seq(),
            // TODO: any other requests for which this can happen?
            _ => true,
        };
        if !consistent {
            self.responder.borrow_mut().respond_to_client(
                message_id,
                Response::Mutation(Err(NdError::InvalidOperation)),
            );
            None
        } else {
            Some(())
        }
    }

    fn check_app_permissions(
        &self,
        app_id: &AppPublicId,
        check: impl FnOnce(AppPermissions) -> bool,
    ) -> Result<(), NdError> {
        if self
            .auth_keys
            .app_permissions(app_id)
            .map(check)
            .unwrap_or(false)
        {
            Ok(())
        } else {
            Err(NdError::AccessDenied)
        }
    }

    fn is_valid_client_signature(
        &self,
        client_id: &PublicId,
        request: &Request,
        message_id: &MessageId,
        signature: &Signature,
    ) -> bool {
        let pub_key = match utils::own_key(client_id) {
            Some(pk) => pk,
            None => {
                error!("{}: Logic error.  This should be unreachable.", self);
                return false;
            }
        };
        match pub_key.verify(signature, utils::serialise(&(request, message_id))) {
            Ok(_) => true,
            Err(error) => {
                warn!(
                    "{}: ({:?}/{:?}) from {} is invalid: {}",
                    self, request, message_id, client_id, error
                );
                false
            }
        }
    }
}

impl Display for Auth {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        write!(formatter, "{}", self.id)
    }
}