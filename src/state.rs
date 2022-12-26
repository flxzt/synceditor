use std::borrow::Cow;
use std::sync::Arc;

use automerge::transaction::{CommitOptions, Transactable};
use automerge::{Automerge, AutomergeError, ObjId, ROOT};
use druid::text::EditableText;
use druid::widget::Controller;
use druid::{
    piet, AppDelegate, Command, DelegateCtx, Env, Event, EventCtx, Handled, Selector, Target,
    Widget,
};
use druid::{Data, Lens};
use futures::channel::mpsc::UnboundedSender;

use crate::connection::{ConnectionMessage, WireMessage};

pub(crate) const SEND_WIRE_MESSAGE: Selector<WireMessage> =
    Selector::new("synceditor.send-wire-message");
pub(crate) const DIAL: Selector<String> = Selector::new("synceditor.dial");
pub(crate) const CHANGE_TOPIC: Selector<String> = Selector::new("synceditor.change-topic");

pub(crate) struct Delegate {
    pub(crate) sender: futures::channel::mpsc::UnboundedSender<ConnectionMessage>,
}

impl<T: Data> AppDelegate<T> for Delegate {
    fn command(
        &mut self,
        _ctx: &mut DelegateCtx,
        _target: Target,
        cmd: &Command,
        _data: &mut T,
        _env: &Env,
    ) -> Handled {
        if let Some(data) = cmd.get(SEND_WIRE_MESSAGE) {
            if let Err(e) = self
                .sender
                .unbounded_send(ConnectionMessage::Send(data.clone()))
            {
                eprintln!("could not send `Send` connection message from app delegate, Err: {e:?}");
            }

            Handled::Yes
        } else if let Some(to_dial) = cmd.get(DIAL) {
            println!("dial: {to_dial}");

            if let Err(e) = self
                .sender
                .unbounded_send(ConnectionMessage::Dial(to_dial.clone()))
            {
                eprintln!("could not send `Dial` connection message from app delegate, Err: {e:?}");
            }
            Handled::Yes
        } else if let Some(topic) = cmd.get(CHANGE_TOPIC) {
            if let Err(e) = self
                .sender
                .unbounded_send(ConnectionMessage::ChangeTopic(topic.clone()))
            {
                eprintln!(
                    "could not send `ChangeTopic` connection message from app delegate, Err: {e:?}"
                );
            }

            Handled::Yes
        } else {
            Handled::No
        }
    }
}

#[derive(Debug, Clone, Data, Lens)]
pub(crate) struct AppState {
    pub(crate) topic: String,
    pub(crate) connect_address: String,
    pub(crate) log: Arc<String>,
    pub(crate) buffer: SyncedBuffer,
}

pub(crate) const BUFFER_TEXT_INIT: &str = "Hello world!";
pub(crate) const TOPIC_NAME_INIT: &str = "testnet";

impl AppState {
    pub(crate) fn new() -> Self {
        Self {
            topic: String::from(TOPIC_NAME_INIT),
            connect_address: String::default(),
            log: Arc::new(String::default()),
            buffer: SyncedBuffer::from_str(BUFFER_TEXT_INIT),
        }
    }

    pub(crate) fn add_log_entry(&mut self, entry: String) {
        println!("{entry}");

        *Arc::make_mut(&mut self.log) += &(String::from("\n") + &entry);
    }

    #[allow(unused)]
    pub(crate) fn add_log_entries(&mut self, entries: Vec<String>) {
        let log = Arc::make_mut(&mut self.log);

        for entry in entries {
            println!("{entry}");

            *log += &(String::from("\n") + &entry);
        }
    }
}

const AM_DOC_BUFFER_NAME: &str = "buffer";

#[derive(Debug, Clone)]
pub(crate) struct SyncedBuffer {
    doc: automerge::Automerge,
    out: String,
}

impl EditableText for SyncedBuffer {
    fn cursor(&self, position: usize) -> Option<druid::text::StringCursor> {
        self.out.cursor(position)
    }

    fn edit(&mut self, range: std::ops::Range<usize>, new: impl Into<String>) {
        let new = new.into();

        // For now we only can do ASCII
        if !new.is_ascii() {
            return;
        }

        let buffer_id = self.buffer_id();

        if let Err(e) = self.doc.transact_with(
            |_| {
                CommitOptions::default().with_message(format!(
                    "replace text: at pos {}, del: {}, text: {}",
                    range.start,
                    range.len(),
                    new
                ))
            },
            |tx| tx.splice_text::<_>(&buffer_id, range.start, range.len(), &new),
        ) {
            eprintln!("automerge transact_with() failed, Err: {e:?}");
        };

        self.update_out();
    }

    fn slice(&self, range: std::ops::Range<usize>) -> Option<std::borrow::Cow<str>> {
        Some(Cow::Borrowed(&self.out[range]))
    }

    fn len(&self) -> usize {
        self.out.len()
    }

    fn prev_word_offset(&self, offset: usize) -> Option<usize> {
        self.out.prev_word_offset(offset)
    }

    fn next_word_offset(&self, offset: usize) -> Option<usize> {
        self.out.next_word_offset(offset)
    }

    fn prev_grapheme_offset(&self, offset: usize) -> Option<usize> {
        self.out.prev_grapheme_offset(offset)
    }

    fn next_grapheme_offset(&self, offset: usize) -> Option<usize> {
        self.out.next_codepoint_offset(offset)
    }

    fn prev_codepoint_offset(&self, offset: usize) -> Option<usize> {
        self.out.prev_codepoint_offset(offset)
    }

    fn next_codepoint_offset(&self, offset: usize) -> Option<usize> {
        self.out.next_codepoint_offset(offset)
    }

    fn preceding_line_break(&self, offset: usize) -> usize {
        self.out.preceding_line_break(offset)
    }

    fn next_line_break(&self, offset: usize) -> usize {
        self.out.next_line_break(offset)
    }

    fn is_empty(&self) -> bool {
        self.out.is_empty()
    }

    fn from_str(s: &str) -> Self {
        let mut doc = automerge::Automerge::new();
        doc.transact_with::<_, _, AutomergeError, _>(
            |_| CommitOptions::default().with_message("init doc with text buffer"),
            |tx| {
                let buffer_id = tx
                    .put_object(ROOT, AM_DOC_BUFFER_NAME, automerge::ObjType::Text)
                    .unwrap();
                tx.splice_text(&buffer_id, 0, 0, s)
            },
        )
        .unwrap();

        Self {
            doc,
            out: String::from(s),
        }
    }
}

impl piet::TextStorage for SyncedBuffer {
    fn as_str(&self) -> &str {
        &self.out
    }
}

impl druid::text::TextStorage for SyncedBuffer {}

impl Data for SyncedBuffer {
    fn same(&self, other: &Self) -> bool {
        self.out.same(&other.out)
    }
}

impl SyncedBuffer {
    pub(crate) fn buffer_id(&self) -> ObjId {
        self.doc.get(ROOT, AM_DOC_BUFFER_NAME).unwrap().unwrap().1
    }

    fn update_out(&mut self) {
        self.out = self.doc.text(&self.buffer_id()).unwrap();
    }

    pub(crate) fn doc_save(&mut self) -> Vec<u8> {
        self.doc.save()
    }

    pub(crate) fn handle_wire_message(
        &mut self,
        msg: WireMessage,
        sender: UnboundedSender<ConnectionMessage>,
    ) -> anyhow::Result<()> {
        match msg {
            WireMessage::MergeDoc(data) => {
                self.doc.merge(&mut Automerge::load(&data)?)?;
            }
            WireMessage::SyncDoc(data) => {
                self.doc.merge(&mut Automerge::load(&data)?)?;

                let doc = self.doc_save();
                sender.unbounded_send(ConnectionMessage::Send(WireMessage::MergeDoc(doc)))?;
            }
            WireMessage::RequestMerge => {
                let doc = self.doc_save();
                sender.unbounded_send(ConnectionMessage::Send(WireMessage::MergeDoc(doc)))?;
            }
        }

        /*
               for change in self.doc.get_changes(&[])? {
                   let length = self.doc.length_at(&self.buffer_id, &[change.hash()]);
                   println!("{}; {}", change.message().unwrap(), length);
               }
               println!("doc str:\n`{}`", self.doc.text(&self.buffer_id)?);
        */

        self.update_out();

        Ok(())
    }
}

/// A controller that sends a message when edits occur
pub(crate) struct SyncedBufferController;

impl<W: Widget<SyncedBuffer>> Controller<SyncedBuffer, W> for SyncedBufferController {
    fn event(
        &mut self,
        child: &mut W,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut SyncedBuffer,
        env: &Env,
    ) {
        let pre_data = data.to_owned();
        child.event(ctx, event, data, env);
        if !data.same(&pre_data) {
            // Send a automerge document
            ctx.submit_command(SEND_WIRE_MESSAGE.with(WireMessage::MergeDoc(data.doc.save())));
        }
    }
}
