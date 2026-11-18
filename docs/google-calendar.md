# Google Calendar and Gmail sign-in

serenebar has no shared OAuth client. To sign in with Google you make one in your own Google Cloud
project. It's free and takes about five minutes.

If you only want to see your events, you don't need any of this. Paste your calendar's secret iCal
link (Google Calendar → Settings → your calendar → *Secret address in iCal format*) into the
**Secret iCal link** field instead.

## 1. Create a project and turn on the API

1. Go to <https://console.cloud.google.com/> and create a project (any name works).
2. Open **APIs & Services → Library**, search for **Google Calendar API** and click **Enable**.
3. For the Gmail unread widget, do the same for **Gmail API**.

## 2. Set up the consent screen

1. Open **APIs & Services → OAuth consent screen** (shown as **Google Auth Platform** in newer
   consoles) and click **Get started**.
2. Give it an app name and your email, and pick **External** for the audience.
3. Under **Audience → Test users**, add the Google account you'll sign in with.

You can leave the app in **Testing**. Google then asks you to sign in again about once a week. To
stop that, click **Publish app**. Google won't review it, because you're the only one using it.

## 3. Create the OAuth client

1. Open **APIs & Services → Credentials → Create credentials → OAuth client ID**.
2. For **Application type**, pick **Desktop app** and give it a name.
3. Copy the **Client ID** and **Client secret**.

A Desktop app client doesn't need a redirect URI. serenebar listens on `http://127.0.0.1` on a
random port, and Google allows that for desktop clients.

## 4. Sign in

1. In serenebar, open **Settings → Feeds & accounts**.
2. Paste the client ID and secret into the two **Google Calendar and Gmail** fields.
3. Click **Sign in with Google**, choose your account in the browser and allow read-only calendar
   access and Gmail labels. If Google warns that the app isn't verified, click **Continue**.

serenebar only asks for `calendar.readonly` and `gmail.labels`. The Gmail one gives the inbox's
unread count and can't read any mail. If you signed in before the Gmail widget existed, sign out
and in again to allow it.

Events come from every calendar you have ticked in Google Calendar's sidebar. The refresh token is encrypted with Windows DPAPI, so only your Windows
account can read it. **Sign out** deletes it.
