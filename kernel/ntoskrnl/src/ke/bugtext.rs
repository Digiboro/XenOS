//! BugCheck message texts (KeGetBugMessageText)
//!
//! Goal: XenOS BSOD strings (not byte-identical to Windows).

#![allow(dead_code)]

/// MessageId из `bugcodes.mc` (NT5).
pub mod bugcheck_message_id {
    /// Intro message (XenOS)
    pub const BUGCHECK_MESSAGE_INTRO: u32 = 0x007F;
    /// Driver line (не используется пока, оставлено для совместимости)
    pub const BUGCODE_ID_DRIVER: u32 = 0x0080;
    /// Next steps header
    pub const PSS_MESSAGE_INTRO: u32 = 0x0081;
    /// Generic recommendations
    pub const BUGCODE_PSS_MESSAGE: u32 = 0x0082;
    /// Technical information header
    pub const BUGCHECK_TECH_INFO: u32 = 0x0083;
}

/// NT5-style KeGetBugMessageText.
///
/// В NT возвращает строку по MessageId из ресурса. У нас — статическая таблица,
/// которую расширяем по мере необходимости.
pub fn ke_get_bug_message_text(message_id: u32) -> Option<&'static str> {
    use bugcheck_message_id::*;
    match message_id {
        BUGCHECK_MESSAGE_INTRO => Some(
            "XenOS detected a fatal system error and halted execution\n\
to prevent further damage.",
        ),
        BUGCODE_ID_DRIVER => Some("The problem may be related to the following module:\n"),
        PSS_MESSAGE_INTRO => Some(
            "If this is the first time you see this screen, restart the system.\n\
If the problem persists, try the following:\n",
        ),
        BUGCODE_PSS_MESSAGE => Some(
            "- Write down the STOP code and parameters shown below.\n\
- If you recently changed kernel code or a driver, revert the change.\n\
- If you added new hardware, disconnect it temporarily.\n\
- If you have a debugger/serial log, attach the output prior to STOP.\n",
        ),
        BUGCHECK_TECH_INFO => Some("Technical information:\n"),
        _ => None,
    }
}
